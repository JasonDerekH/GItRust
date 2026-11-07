//! Storage for compressed Git loose objects.

use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use flate2::write::ZlibEncoder;
use flate2::Compression;

use crate::hash::{git_object_sha1, to_hex};
use crate::paths::common_metadata;

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A decoded Git object.
#[derive(Debug, PartialEq, Eq)]
pub struct Object {
    pub object_type: String,
    pub content: Vec<u8>,
}

/// Write an object to the repository in the current directory.
pub fn write_object(object_type: &str, content: &[u8]) -> io::Result<String> {
    let object_id = to_hex(&git_object_sha1(object_type, content));
    let path = object_path(&object_id)?;

    if path.exists() {
        return Ok(object_id);
    }

    let Some(parent) = path.parent() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "object path has no parent directory",
        ));
    };
    fs::create_dir_all(parent)?;

    let header = format!("{object_type} {}\0", content.len());
    let mut object = Vec::with_capacity(header.len() + content.len());
    object.extend_from_slice(header.as_bytes());
    object.extend_from_slice(content);

    let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temporary_path = parent.join(format!("tmp-{}-{counter}", std::process::id()));

    let write_result = (|| -> io::Result<()> {
        let temporary_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)?;
        let mut encoder = ZlibEncoder::new(temporary_file, Compression::default());
        encoder.write_all(&object)?;
        encoder.finish()?.sync_all()?;

        match fs::rename(&temporary_path, &path) {
            Ok(()) => Ok(()),
            Err(_error) if path.exists() => {
                let _ = fs::remove_file(&temporary_path);
                Ok(())
            }
            Err(error) => Err(error),
        }
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }

    write_result.map(|()| object_id)
}

/// Read and decompress an object from the repository in the current directory.
pub fn read_object(object_id: &str) -> io::Result<Object> {
    let path = object_path(object_id)?;
    let compressed = fs::read(path)?;
    let mut decoder = flate2::read::ZlibDecoder::new(compressed.as_slice());
    let mut object = Vec::new();
    decoder.read_to_end(&mut object)?;

    let Some(header_end) = object.iter().position(|&byte| byte == 0) else {
        return Err(invalid_object("object header has no terminator"));
    };
    let header = std::str::from_utf8(&object[..header_end])
        .map_err(|_| invalid_object("object header is not valid UTF-8"))?;
    let Some((object_type, length)) = header.split_once(' ') else {
        return Err(invalid_object("object header is malformed"));
    };
    let expected_length: usize = length
        .parse()
        .map_err(|_| invalid_object("object length is malformed"))?;
    let content = object[header_end + 1..].to_vec();

    if expected_length != content.len() {
        return Err(invalid_object("object length does not match content"));
    }

    Ok(Object {
        object_type: object_type.to_owned(),
        content,
    })
}

/// Check whether an object exists in the repository.
pub fn object_exists(object_id: &str) -> bool {
    object_path(object_id)
        .map(|path| path.is_file())
        .unwrap_or(false)
}

fn object_path(object_id: &str) -> io::Result<PathBuf> {
    if object_id.len() != 40
        || !object_id
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "object id must contain exactly 40 hexadecimal characters",
        ));
    }

    Ok(common_metadata()?
        .join("objects")
        .join(&object_id[..2])
        .join(&object_id[2..]))
}

fn invalid_object(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
