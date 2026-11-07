//! Repository-aware Windows path handling.
//!
//! Symlinks, submodules, executable bits, and unusual Windows reparse points
//! are deliberately deferred. Canonicalization keeps this basic file reader
//! from accepting a path that escapes the repository.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Find the nearest repository root by looking for a `.rustgit` directory.
pub fn repository_root() -> io::Result<PathBuf> {
    let mut directory = std::env::current_dir()?;

    loop {
        if directory.join(".rustgit").is_dir() {
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

#[cfg(windows)]
fn comparable_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/").to_lowercase()
}
