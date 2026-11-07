//! Hashing helpers used by the Git prototype.

use sha1::{Digest, Sha1};
use std::fs;
use std::io;

/// Read an entire file into memory.
pub fn read_file(path: impl AsRef<std::path::Path>) -> io::Result<Vec<u8>> {
    fs::read(path)
}

/// Hash arbitrary bytes with SHA-1.
pub fn sha1_bytes(bytes: &[u8]) -> [u8; 20] {
    let digest = Sha1::digest(bytes);
    let mut result = [0u8; 20];
    result.copy_from_slice(&digest);
    result
}

/// Format bytes as lowercase hexadecimal.
pub fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);

    for &byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }

    output
}

/// Hash bytes using Git's object format.
pub fn git_object_sha1(object_type: &str, content: &[u8]) -> [u8; 20] {
    let header = format!("{object_type} {}\0", content.len());
    let mut object = Vec::with_capacity(header.len() + content.len());
    object.extend_from_slice(header.as_bytes());
    object.extend_from_slice(content);
    sha1_bytes(&object)
}

/// Return a Git-compatible object hash as lowercase hexadecimal.
pub fn git_object_sha1_hex(object_type: &str, content: &[u8]) -> String {
    to_hex(&git_object_sha1(object_type, content))
}

//I used ChatGPT to help me develop tests for the code and keep me on a right track

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_arbitrary_bytes() {
        assert_eq!(
            to_hex(&sha1_bytes(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
    }

    #[test]
    fn hashes_git_objects() {
        assert_eq!(
            git_object_sha1_hex("blob", b"hello"),
            "b6fc4c620b67d95f953a5c1c1230aaab5db5a1b0"
        );
    }
}
