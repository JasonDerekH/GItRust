//! Commit objects and the current branch reference.

use std::fs;
use std::io;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::object_store::{read_object, write_object};
use crate::paths::repository_root;

/// Identity information written to an author or committer header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    pub name: String,
    pub email: String,
    pub timestamp: i64,
    pub timezone: String,
}

impl Signature {
    pub fn now(name: impl Into<String>, email: impl Into<String>) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        Self {
            name: name.into(),
            email: email.into(),
            timestamp,
            timezone: "+0000".to_owned(),
        }
    }

    fn as_header(&self) -> String {
        format!(
            "{} <{}> {} {}",
            self.name, self.email, self.timestamp, self.timezone
        )
    }
}

/// A decoded commit object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub tree_id: String,
    pub parents: Vec<String>,
    pub author: String,
    pub committer: String,
    pub message: String,
}

/// Create a commit and update the branch referenced by `HEAD`.
pub fn commit(
    tree_id: &str,
    parents: &[String],
    author: &Signature,
    message: &str,
) -> io::Result<String> {
    let mut content = format!("tree {tree_id}\n");
    for parent in parents {
        content.push_str(&format!("parent {parent}\n"));
    }
    let author_header = author.as_header();
    content.push_str(&format!("author {author_header}\n"));
    content.push_str(&format!("committer {author_header}\n\n"));
    content.push_str(message);
    if !message.ends_with('\n') {
        content.push('\n');
    }

    let commit_id = write_object("commit", content.as_bytes())?;
    update_head(&commit_id)?;
    Ok(commit_id)
}

/// Read and parse a commit object.
pub fn read_commit(commit_id: &str) -> io::Result<Commit> {
    let object = read_object(commit_id)?;
    if object.object_type != "commit" {
        return Err(invalid_commit("object is not a commit"));
    }

    let text = std::str::from_utf8(&object.content)
        .map_err(|_| invalid_commit("commit content is not valid UTF-8"))?;
    let (headers, message) = text
        .split_once("\n\n")
        .ok_or_else(|| invalid_commit("commit has no message separator"))?;

    let mut tree_id = None;
    let mut parents = Vec::new();
    let mut author = None;
    let mut committer = None;
    for line in headers.lines() {
        if let Some(value) = line.strip_prefix("tree ") {
            tree_id = Some(value.to_owned());
        } else if let Some(value) = line.strip_prefix("parent ") {
            parents.push(value.to_owned());
        } else if let Some(value) = line.strip_prefix("author ") {
            author = Some(value.to_owned());
        } else if let Some(value) = line.strip_prefix("committer ") {
            committer = Some(value.to_owned());
        }
    }

    Ok(Commit {
        tree_id: tree_id.ok_or_else(|| invalid_commit("commit has no tree"))?,
        parents,
        author: author.ok_or_else(|| invalid_commit("commit has no author"))?,
        committer: committer.ok_or_else(|| invalid_commit("commit has no committer"))?,
        message: message.strip_suffix('\n').unwrap_or(message).to_owned(),
    })
}

/// Return the current commit, if the current branch has one.
pub fn head_commit() -> io::Result<Option<String>> {
    let reference = head_reference()?;
    let path = repository_root()?.join(".rustgit").join(reference);
    if !path.exists() {
        return Ok(None);
    }
    let commit_id = fs::read_to_string(path)?.trim().to_owned();
    if commit_id.is_empty() {
        Ok(None)
    } else {
        Ok(Some(commit_id))
    }
}

fn update_head(commit_id: &str) -> io::Result<()> {
    let reference = head_reference()?;
    let path = repository_root()?.join(".rustgit").join(reference);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, format!("{commit_id}\n"))
}

fn head_reference() -> io::Result<String> {
    let head = fs::read_to_string(repository_root()?.join(".rustgit").join("HEAD"))?;
    let reference = head
        .trim()
        .strip_prefix("ref: ")
        .ok_or_else(|| invalid_commit("HEAD is not a symbolic reference"))?;
    if !reference.starts_with("refs/heads/") {
        return Err(invalid_commit("HEAD does not point to a branch"));
    }
    Ok(reference.to_owned())
}

fn invalid_commit(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
