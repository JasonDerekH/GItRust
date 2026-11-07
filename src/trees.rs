//! Git tree snapshots for regular files and directories.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use crate::hash::to_hex;
use crate::index::IndexEntry;
use crate::object_store::{read_object, write_object};

const BLOB_MODE: u32 = 0o100644;
const TREE_MODE: u32 = 0o40000;

/// One entry in a tree object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    pub mode: u32,
    pub name: String,
    pub object_id: String,
}

impl TreeEntry {
    pub fn is_tree(&self) -> bool {
        self.mode == TREE_MODE
    }
}

/// Recursively snapshot a directory and return its tree object ID.
pub fn write_tree(directory: impl AsRef<Path>) -> io::Result<String> {
    let directory = directory.as_ref();
    let mut entries = Vec::new();

    for item in fs::read_dir(directory)? {
        let item = item?;
        let name = item.file_name().to_string_lossy().into_owned();

        if name == ".rustgit" {
            continue;
        }

        let path = item.path();
        let file_type = item.file_type()?;
        let (mode, object_id) = if file_type.is_file() {
            (BLOB_MODE, write_object("blob", &fs::read(path)?)?)
        } else if file_type.is_dir() {
            (TREE_MODE, write_tree(path)?)
        } else {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!("unsupported filesystem entry: {}", item.path().display()),
            ));
        };

        entries.push(TreeEntry {
            mode,
            name,
            object_id,
        });
    }

    write_entries(entries)
}

/// Write a tree from the flat paths stored in the staging index.
pub fn write_tree_from_index(entries: &[IndexEntry]) -> io::Result<String> {
    let mut root = TreeNode::default();

    for entry in entries {
        let parts: Vec<&str> = entry.path.split('/').collect();
        if parts
            .iter()
            .any(|part| part.is_empty() || *part == "." || *part == "..")
        {
            return Err(invalid_tree("index path is not normalized"));
        }
        root.insert(&parts, entry)?;
    }

    root.write()
}

/// Read a tree object and decode its entries.
pub fn read_tree(tree_id: &str) -> io::Result<Vec<TreeEntry>> {
    let object = read_object(tree_id)?;
    if object.object_type != "tree" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "object is not a tree",
        ));
    }

    let mut entries = Vec::new();
    let mut cursor = 0;
    while cursor < object.content.len() {
        let mode_end = find_byte(&object.content, cursor, b' ')?;
        let mode = std::str::from_utf8(&object.content[cursor..mode_end])
            .map_err(|_| invalid_tree("tree mode is not valid UTF-8"))?
            .parse::<u32>()
            .map_err(|_| invalid_tree("tree mode is invalid"))?;
        cursor = mode_end + 1;

        let name_end = find_byte(&object.content, cursor, 0)?;
        let name = String::from_utf8(object.content[cursor..name_end].to_vec())
            .map_err(|_| invalid_tree("tree name is not valid UTF-8"))?;
        cursor = name_end + 1;

        if cursor + 20 > object.content.len() {
            return Err(invalid_tree("tree entry is missing its object id"));
        }
        let object_id = to_hex(&object.content[cursor..cursor + 20]);
        cursor += 20;

        entries.push(TreeEntry {
            mode,
            name,
            object_id,
        });
    }

    Ok(entries)
}

fn write_entries(mut entries: Vec<TreeEntry>) -> io::Result<String> {
    entries.sort_by(|left, right| tree_sort_key(left).cmp(&tree_sort_key(right)));

    let mut content = Vec::new();
    for entry in &entries {
        content.extend_from_slice(entry.mode.to_string().as_bytes());
        content.push(b' ');
        content.extend_from_slice(entry.name.as_bytes());
        content.push(0);
        content.extend_from_slice(&decode_hex_id(&entry.object_id)?);
    }

    write_object("tree", &content)
}

#[derive(Default)]
struct TreeNode {
    files: Vec<TreeEntry>,
    directories: BTreeMap<String, TreeNode>,
}

impl TreeNode {
    fn insert(&mut self, parts: &[&str], entry: &IndexEntry) -> io::Result<()> {
        if parts.len() == 1 {
            if self.directories.contains_key(parts[0])
                || self.files.iter().any(|file| file.name == parts[0])
            {
                return Err(invalid_tree("index contains conflicting paths"));
            }
            self.files.push(TreeEntry {
                mode: entry.mode,
                name: parts[0].to_owned(),
                object_id: entry.object_id.clone(),
            });
            return Ok(());
        }

        if self.files.iter().any(|file| file.name == parts[0]) {
            return Err(invalid_tree("index contains conflicting paths"));
        }
        self.directories
            .entry(parts[0].to_owned())
            .or_default()
            .insert(&parts[1..], entry)
    }

    fn write(self) -> io::Result<String> {
        let mut entries = self.files;
        for (name, directory) in self.directories {
            entries.push(TreeEntry {
                mode: TREE_MODE,
                name,
                object_id: directory.write()?,
            });
        }
        write_entries(entries)
    }
}

fn tree_sort_key(entry: &TreeEntry) -> Vec<u8> {
    let mut key = entry.name.as_bytes().to_vec();
    if entry.is_tree() {
        key.push(b'/');
    }
    key
}

fn decode_hex_id(object_id: &str) -> io::Result<Vec<u8>> {
    if object_id.len() != 40 {
        return Err(invalid_tree(
            "object id must contain 40 hexadecimal characters",
        ));
    }

    (0..object_id.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&object_id[index..index + 2], 16)
                .map_err(|_| invalid_tree("object id is not hexadecimal"))
        })
        .collect()
}

fn find_byte(bytes: &[u8], start: usize, target: u8) -> io::Result<usize> {
    bytes[start..]
        .iter()
        .position(|&byte| byte == target)
        .map(|offset| start + offset)
        .ok_or_else(|| invalid_tree("tree entry is malformed"))
}

fn invalid_tree(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_sorting_is_independent_of_enumeration_order() {
        let blob_a = "0000000000000000000000000000000000000001";
        let blob_b = "0000000000000000000000000000000000000002";
        let entries_a = vec![
            TreeEntry {
                mode: TREE_MODE,
                name: "src".to_owned(),
                object_id: blob_a.to_owned(),
            },
            TreeEntry {
                mode: BLOB_MODE,
                name: "src-file".to_owned(),
                object_id: blob_b.to_owned(),
            },
        ];
        let entries_b = entries_a.iter().cloned().rev().collect();

        assert_eq!(
            tree_content(entries_a).unwrap(),
            tree_content(entries_b).unwrap()
        );
    }

    fn tree_content(mut entries: Vec<TreeEntry>) -> io::Result<Vec<u8>> {
        entries.sort_by(|left, right| tree_sort_key(left).cmp(&tree_sort_key(right)));
        let mut content = Vec::new();
        for entry in entries {
            content.extend_from_slice(entry.mode.to_string().as_bytes());
            content.push(b' ');
            content.extend_from_slice(entry.name.as_bytes());
            content.push(0);
            content.extend_from_slice(&decode_hex_id(&entry.object_id)?);
        }
        Ok(content)
    }
}
