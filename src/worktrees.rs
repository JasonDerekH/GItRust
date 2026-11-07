//! Linked working trees that share objects and references.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::commits::read_commit;
use crate::hash::{sha1_bytes, to_hex};
use crate::index::{write_index_at, IndexEntry};
use crate::object_store::read_object;
use crate::paths::{common_metadata, worktree_metadata};
use crate::refs::{branch_reference, resolve_ref};
use crate::trees::read_tree;

/// Create a linked working tree at `path` for an existing branch.
pub fn add(path: impl AsRef<Path>, branch: &str) -> io::Result<()> {
    let common = common_metadata()?;
    let main_root = common
        .parent()
        .ok_or_else(|| invalid_worktree("shared metadata has no repository root"))?
        .to_path_buf();
    let reference = branch_reference(branch)?;
    if branch_in_use(&common, &reference)? {
        return Err(invalid_worktree("branch is already checked out"));
    }

    let commit_id = resolve_ref(&reference)?;
    let commit = read_commit(&commit_id)?;
    let entries = flatten_tree(&commit.tree_id)?;

    let target = absolute_target(path.as_ref())?;
    if target == main_root || target.starts_with(&main_root) {
        return Err(invalid_worktree(
            "linked worktree must be outside the current repository",
        ));
    }
    if target.exists() {
        if !target.is_dir() || fs::read_dir(&target)?.next().is_some() {
            return Err(invalid_worktree(
                "linked worktree path must be a new or empty directory",
            ));
        }
    } else {
        fs::create_dir_all(&target)?;
    }
    let target = fs::canonicalize(target)?;

    let worktree_id = &to_hex(&sha1_bytes(target.to_string_lossy().as_bytes()))[..16];
    let metadata = common.join("worktrees").join(worktree_id);
    if metadata.exists() {
        return Err(invalid_worktree("worktree metadata already exists"));
    }
    fs::create_dir_all(&metadata)?;

    fs::write(
        metadata.join("commondir"),
        format!("{}\n", common.display()),
    )?;
    fs::write(metadata.join("worktree"), format!("{}\n", target.display()))?;
    fs::write(metadata.join("HEAD"), format!("ref: {reference}\n"))?;
    write_index_at(&metadata, &entries)?;

    for entry in &entries {
        let object = read_object(&entry.object_id)?;
        if object.object_type != "blob" {
            return Err(invalid_worktree("tree entry does not point to a blob"));
        }
        let file = target.join(&entry.path);
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(file, object.content)?;
    }

    fs::write(
        target.join(".rustgit"),
        format!("gitdir: {}\n", metadata.display()),
    )?;

    Ok(())
}

/// Check whether another worktree already owns a branch.
pub fn branch_in_use_elsewhere(requested: &str) -> io::Result<bool> {
    let common = common_metadata()?;
    let current = worktree_metadata()?;

    if common != current && head_uses_branch(&common.join("HEAD"), requested)? {
        return Ok(true);
    }

    let worktrees = common.join("worktrees");
    if !worktrees.exists() {
        return Ok(false);
    }
    for item in fs::read_dir(worktrees)? {
        let item = item?;
        if item.file_type()?.is_dir()
            && item.path() != current
            && head_uses_branch(&item.path().join("HEAD"), requested)?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn branch_in_use(common: &Path, requested: &str) -> io::Result<bool> {
    if head_uses_branch(&common.join("HEAD"), requested)? {
        return Ok(true);
    }

    let worktrees = common.join("worktrees");
    if !worktrees.exists() {
        return Ok(false);
    }
    for item in fs::read_dir(worktrees)? {
        let item = item?;
        if item.file_type()?.is_dir() && head_uses_branch(&item.path().join("HEAD"), requested)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn head_uses_branch(head: &Path, requested: &str) -> io::Result<bool> {
    if !head.exists() {
        return Ok(false);
    }
    let value = fs::read_to_string(head)?;
    Ok(value.trim() == format!("ref: {requested}"))
}

fn absolute_target(path: &Path) -> io::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
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

fn invalid_worktree(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
