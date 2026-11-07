//! A small, text-based staging index.

use std::fs;
use std::io;
use std::path::Path;

use crate::object_store::write_object;
use crate::paths::{read_repository_file, worktree_metadata};

pub const FILE_MODE: u32 = 0o100644;

/// One staged file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    pub path: String,
    pub mode: u32,
    pub object_id: String,
}

/// Add a file or directory to the index.
pub fn add(path: impl AsRef<Path>) -> io::Result<()> {
    let path = path.as_ref();
    let mut entries = read_index()?;
    let mut files = Vec::new();
    collect_files(path, &mut files)?;

    for file in files {
        let (normalized_path, content) = read_repository_file(&file)?;
        let object_id = write_object("blob", &content)?;
        entries.retain(|entry| entry.path != normalized_path);
        entries.push(IndexEntry {
            path: normalized_path,
            mode: FILE_MODE,
            object_id,
        });
    }

    write_index(&entries)
}

/// Read the custom index from `.rustgit/index`.
pub fn read_index() -> io::Result<Vec<IndexEntry>> {
    read_index_at(worktree_metadata()?.join("index"))
}

/// Read an index from a specific worktree metadata directory.
pub fn read_index_at(path: impl AsRef<Path>) -> io::Result<Vec<IndexEntry>> {
    let index_path = path.as_ref().to_owned();
    if !index_path.exists() {
        return Ok(Vec::new());
    }

    let contents = fs::read_to_string(index_path)?;
    let lines: Vec<&str> = contents.lines().collect();
    if lines.len() % 3 != 0 {
        return Err(invalid_index(
            "index must contain path, mode, and object-id lines",
        ));
    }

    let mut entries = Vec::new();
    for fields in lines.chunks(3) {
        let mode = u32::from_str_radix(fields[1], 8)
            .map_err(|_| invalid_index("index mode is invalid"))?;
        if fields[0].is_empty()
            || fields[0].starts_with('/')
            || fields[0].contains("\\")
            || fields[2].len() != 40
            || !fields[2]
                .chars()
                .all(|character| character.is_ascii_hexdigit())
        {
            return Err(invalid_index("index entry is invalid"));
        }
        entries.push(IndexEntry {
            path: fields[0].to_owned(),
            mode,
            object_id: fields[2].to_owned(),
        });
    }

    entries.sort_by(|left, right| left.path.as_bytes().cmp(right.path.as_bytes()));
    Ok(entries)
}

/// Write index entries in deterministic path order.
pub fn write_index(entries: &[IndexEntry]) -> io::Result<()> {
    write_index_at(worktree_metadata()?, entries)
}

/// Write an index into a specific worktree metadata directory.
pub fn write_index_at(metadata: impl AsRef<Path>, entries: &[IndexEntry]) -> io::Result<()> {
    let mut entries = entries.to_vec();
    entries.sort_by(|left, right| left.path.as_bytes().cmp(right.path.as_bytes()));

    let mut contents = String::new();
    for entry in entries {
        contents.push_str(&entry.path);
        contents.push('\n');
        contents.push_str(&format!("{:o}\n", entry.mode));
        contents.push_str(&entry.object_id);
        contents.push('\n');
    }

    let index_path = metadata.as_ref().join("index");
    fs::write(index_path, contents)
}

fn collect_files(path: &Path, files: &mut Vec<std::path::PathBuf>) -> io::Result<()> {
    let metadata = fs::metadata(path)?;
    if metadata.is_file() {
        files.push(path.to_path_buf());
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "add supports regular files and directories only",
        ));
    }

    for item in fs::read_dir(path)? {
        let item = item?;
        if item.file_name() == ".rustgit" {
            continue;
        }
        collect_files(&item.path(), files)?;
    }
    Ok(())
}

fn invalid_index(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
