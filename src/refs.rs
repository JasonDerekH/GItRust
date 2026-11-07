use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::object_store::object_exists;
use crate::paths::{common_metadata, worktree_metadata};

pub fn update_ref(name: &str, object_id: &str) -> io::Result<()> {
    let reference = normalize_reference(name)?;
    if object_id.len() != 40
        || !object_id
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(invalid_reference(
            "object id must contain 40 hexadecimal characters",
        ));
    }
    if !object_exists(object_id) {
        return Err(invalid_reference("object does not exist"));
    }

    let path = reference_path(&reference)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, format!("{object_id}\n"))
}

pub fn resolve_ref(name: &str) -> io::Result<String> {
    resolve_ref_at(name, 0)
}

pub fn list_refs() -> io::Result<Vec<(String, String)>> {
    let root = common_metadata()?.join("refs");
    let mut names = Vec::new();
    collect_refs(&root, &root, &mut names)?;
    names.sort();

    names
        .into_iter()
        .map(|name| Ok((name.clone(), resolve_ref(&name)?)))
        .collect()
}

pub fn current_branch() -> io::Result<Option<String>> {
    let head = fs::read_to_string(worktree_metadata()?.join("HEAD"))?;
    let Some(reference) = head.trim().strip_prefix("ref: refs/heads/") else {
        return Ok(None);
    };
    Ok(Some(reference.to_owned()))
}

pub fn branch_reference(name: &str) -> io::Result<String> {
    if let Some(reference) = name.strip_prefix("refs/heads/") {
        validate_reference_part(reference)?;
        Ok(format!("refs/heads/{reference}"))
    } else {
        validate_reference_part(name)?;
        Ok(format!("refs/heads/{name}"))
    }
}

fn resolve_ref_at(name: &str, depth: usize) -> io::Result<String> {
    if depth > 8 {
        return Err(invalid_reference("reference chain is too deep"));
    }

    let reference = if name == "HEAD" {
        "HEAD".to_owned()
    } else {
        normalize_reference(name)?
    };
    let path = reference_path(&reference)?;
    let value = fs::read_to_string(path)?.trim().to_owned();

    if let Some(next) = value.strip_prefix("ref: ") {
        return resolve_ref_at(next, depth + 1);
    }
    if value.len() != 40 || !value.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(invalid_reference("reference does not contain an object id"));
    }
    Ok(value)
}

fn normalize_reference(name: &str) -> io::Result<String> {
    if name == "HEAD" {
        return Ok("HEAD".to_owned());
    }
    if name.starts_with("refs/") {
        validate_reference_part(name)?;
        Ok(name.to_owned())
    } else {
        branch_reference(name)
    }
}

fn validate_reference_part(name: &str) -> io::Result<()> {
    if name.is_empty()
        || name.starts_with('/')
        || name.contains('\\')
        || name.contains("..")
        || name.ends_with('/')
    {
        return Err(invalid_reference("invalid reference name"));
    }
    Ok(())
}

fn reference_path(name: &str) -> io::Result<PathBuf> {
    if name == "HEAD" {
        return Ok(worktree_metadata()?.join("HEAD"));
    }
    Ok(common_metadata()?.join(name))
}

fn collect_refs(root: &Path, directory: &Path, names: &mut Vec<String>) -> io::Result<()> {
    if !directory.exists() {
        return Ok(());
    }
    for item in fs::read_dir(directory)? {
        let item = item?;
        if item.file_type()?.is_dir() {
            collect_refs(root, &item.path(), names)?;
        } else if item.file_type()?.is_file() {
            let item_path = item.path();
            let relative = item_path
                .strip_prefix(root)
                .map_err(|_| invalid_reference("could not list reference"))?;
            names.push(format!(
                "refs/{}",
                relative.to_string_lossy().replace('\\', "/")
            ));
        }
    }
    Ok(())
}

fn invalid_reference(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
