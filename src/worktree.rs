//! Ordinary single-worktree checkout.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::Path;

use crate::commits::read_commit;
use crate::hash::git_object_sha1_hex;
use crate::index::{read_index, write_index, IndexEntry};
use crate::object_store::{object_exists, read_object};
use crate::paths::repository_root;
use crate::refs::{branch_reference, resolve_ref};
use crate::trees::read_tree;

/// Check out a commit ID or a branch name into the current working directory.
pub fn checkout(target: &str) -> io::Result<()> {
    let repository = repository_root()?;
    let resolved = resolve_target(&repository, target)?;
    let target_entries = flatten_tree(&resolved.commit.tree_id)?;
    check_safety(&repository, &read_index()?, &target_entries)?;

    let current_entries = read_index()?;
    let target_paths: HashSet<&str> = target_entries
        .iter()
        .map(|entry| entry.path.as_str())
        .collect();

    for entry in &current_entries {
        if !target_paths.contains(entry.path.as_str()) {
            let path = repository.join(&entry.path);
            if path.exists() {
                fs::remove_file(path)?;
            }
        }
    }

    for entry in &target_entries {
        let object = read_object(&entry.object_id)?;
        if object.object_type != "blob" {
            return Err(invalid_checkout("tree entry does not point to a blob"));
        }
        let path = repository.join(&entry.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, object.content)?;
    }

    fs::write(
        repository.join(".rustgit").join("HEAD"),
        resolved.head_value,
    )?;
    write_index(&target_entries)
}

struct ResolvedTarget {
    commit: crate::commits::Commit,
    head_value: String,
}

fn resolve_target(_repository: &Path, target: &str) -> io::Result<ResolvedTarget> {
    if object_exists(target) {
        return Ok(ResolvedTarget {
            commit: read_commit(target)?,
            head_value: format!("{target}\n"),
        });
    }

    let reference = branch_reference(target)?;
    let commit_id = resolve_ref(&reference)?;

    Ok(ResolvedTarget {
        commit: read_commit(&commit_id)?,
        head_value: format!("ref: {reference}\n"),
    })
}

fn flatten_tree(tree_id: &str) -> io::Result<Vec<IndexEntry>> {
    let mut entries = Vec::new();
    flatten_tree_at(tree_id, "", &mut entries)?;
    entries.sort_by(|left, right| left.path.as_bytes().cmp(right.path.as_bytes()));
    Ok(entries)
}

fn flatten_tree_at(tree_id: &str, prefix: &str, entries: &mut Vec<IndexEntry>) -> io::Result<()> {
    for entry in read_tree(tree_id)? {
        let path = if prefix.is_empty() {
            entry.name.clone()
        } else {
            format!("{prefix}/{}", entry.name)
        };

        if entry.is_tree() {
            flatten_tree_at(&entry.object_id, &path, entries)?;
        } else {
            entries.push(IndexEntry {
                path,
                mode: entry.mode,
                object_id: entry.object_id,
            });
        }
    }
    Ok(())
}

fn check_safety(
    repository: &Path,
    current: &[IndexEntry],
    target: &[IndexEntry],
) -> io::Result<()> {
    let current_by_path: HashMap<&str, &IndexEntry> = current
        .iter()
        .map(|entry| (entry.path.as_str(), entry))
        .collect();

    for entry in current {
        let path = repository.join(&entry.path);
        if path.exists() {
            if !path.is_file() {
                return Err(invalid_checkout("a tracked path is not a regular file"));
            }
            let actual = git_object_sha1_hex("blob", &fs::read(path)?);
            if actual != entry.object_id {
                return Err(invalid_checkout(
                    "checkout would overwrite a modified tracked file",
                ));
            }
        }
    }

    for entry in target {
        let path = repository.join(&entry.path);
        if path.exists() && !current_by_path.contains_key(entry.path.as_str()) {
            return Err(invalid_checkout(
                "checkout would overwrite an untracked file",
            ));
        }

        let mut parent = path.parent();
        while let Some(directory) = parent {
            if directory == repository {
                break;
            }
            if directory.is_file() {
                return Err(invalid_checkout(
                    "checkout path is blocked by an existing file",
                ));
            }
            parent = directory.parent();
        }
    }

    Ok(())
}

fn invalid_checkout(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
