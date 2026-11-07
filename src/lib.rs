//! Small building blocks for the Git prototype.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub mod commits;
pub mod hash;
pub mod index;
pub mod object_store;
pub mod paths;
pub mod refs;
pub mod trees;
pub mod worktree;

/// Create a Git repository in the current directory.
pub fn init() -> io::Result<()> {
    init_at(".")
}

/// Create a RustGit repository inside `directory`.
pub fn init_at(directory: impl AsRef<Path>) -> io::Result<()> {
    let git_directory = directory.as_ref().join(".rustgit");

    fs::create_dir_all(git_directory.join("objects"))?;
    fs::create_dir_all(git_directory.join("refs").join("heads"))?;
    fs::write(git_directory.join("HEAD"), "ref: refs/heads/main\n")?;
    fs::write(
        git_directory.join("config"),
        "[core]\n\trepositoryformatversion = 0\n\tbare = false\n",
    )?;
    fs::write(git_directory.join("index"), [])?;

    Ok(())
}

/// Find the path for a loose Git object identified by its hexadecimal hash.
pub fn object_path(repository: impl AsRef<Path>, hash: &str) -> io::Result<PathBuf> {
    if hash.len() < 3 || !hash.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "an object hash must contain at least three hexadecimal characters",
        ));
    }

    Ok(repository
        .as_ref()
        .join(".rustgit")
        .join("objects")
        .join(&hash[..2])
        .join(&hash[2..]))
}
