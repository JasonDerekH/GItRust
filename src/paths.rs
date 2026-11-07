//! Repository-aware Windows path handling.
//!
//! Symlinks, submodules, executable bits, and unusual Windows reparse points
//! are deliberately deferred. Canonicalization keeps this basic file reader
//! from accepting a path that escapes the repository.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Find the nearest working-tree root by looking for `.rustgit` metadata.
pub fn repository_root() -> io::Result<PathBuf> {
    let mut directory = std::env::current_dir()?;

    loop {
        let metadata = directory.join(".rustgit");
        if metadata.is_dir() || metadata.is_file() {
            return fs::canonicalize(directory);
        }

        if !directory.pop() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "not inside a RustGit repository",
            ));
        }
    }
}

/// Return the metadata directory for the current working tree.
pub fn worktree_metadata() -> io::Result<PathBuf> {
    let metadata = repository_root()?.join(".rustgit");
    if metadata.is_dir() {
        return fs::canonicalize(metadata);
    }

    let marker = fs::read_to_string(metadata)?;
    let target = marker
        .trim()
        .strip_prefix("gitdir: ")
        .ok_or_else(|| invalid_path("linked worktree metadata marker is invalid"))?;
    fs::canonicalize(target).map_err(Into::into)
}

/// Return the shared repository metadata directory.
pub fn common_metadata() -> io::Result<PathBuf> {
    let worktree = worktree_metadata()?;
    let commondir = worktree.join("commondir");
    if !commondir.exists() {
        return Ok(worktree);
    }

    let value = fs::read_to_string(commondir)?;
    let path = Path::new(value.trim());
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        worktree.join(path)
    };
    fs::canonicalize(path).map_err(Into::into)
}

/// Resolve a file and return its repository-relative Git-style path.
pub fn normalize_repository_path(path: impl AsRef<Path>) -> io::Result<String> {
    let repository = repository_root()?;
    let input = path.as_ref();
    let absolute = if input.is_absolute() {
        input.to_path_buf()
    } else {
        std::env::current_dir()?.join(input)
    };
    let absolute = fs::canonicalize(absolute)?;

    if !is_inside(&repository, &absolute) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "file is outside the RustGit repository",
        ));
    }

    if !absolute.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "hash-object requires a file",
        ));
    }

    let relative = absolute
        .strip_prefix(&repository)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "could not make path relative"))?;

    // Git tree paths always use forward slashes, including on Windows.
    Ok(relative.to_string_lossy().replace('\\', "/"))
}

/// Resolve and read a repository file, returning its normalized Git path.
pub fn read_repository_file(path: impl AsRef<Path>) -> io::Result<(String, Vec<u8>)> {
    let normalized_path = normalize_repository_path(path.as_ref())?;
    let input = path.as_ref();
    let absolute = if input.is_absolute() {
        input.to_path_buf()
    } else {
        std::env::current_dir()?.join(input)
    };

    Ok((normalized_path, fs::read(absolute)?))
}

fn is_inside(repository: &Path, path: &Path) -> bool {
    if path.starts_with(repository) {
        return true;
    }

    #[cfg(windows)]
    {
        let root = comparable_path(repository);
        let candidate = comparable_path(path);
        return candidate == root || candidate.starts_with(&(root + "/"));
    }

    #[cfg(not(windows))]
    false
}

fn invalid_path(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(windows)]
fn comparable_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/").to_lowercase()
}
