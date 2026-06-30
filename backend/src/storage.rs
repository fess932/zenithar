//! Blob storage behind a trait so the on-disk backend can be swapped for S3/MinIO
//! later without touching callers. Methods are synchronous (plain file IO) and are
//! expected to be called from `spawn_blocking`.

use std::io;
use std::path::PathBuf;

pub trait Storage: Send + Sync {
    /// Store bytes under `key` (overwriting). `key` must be a sanitized leaf name.
    fn put(&self, key: &str, bytes: &[u8]) -> io::Result<()>;
    /// Read bytes for `key`, or `None` if absent.
    fn get(&self, key: &str) -> io::Result<Option<Vec<u8>>>;
    /// Delete `key` if present (a missing key is not an error).
    fn remove(&self, key: &str) -> io::Result<()>;
}

/// Files under a single directory. The key is the file name; we reject anything
/// that isn't a plain `[A-Za-z0-9._-]` leaf so a key can never escape the root.
pub struct DiskStorage {
    root: PathBuf,
}

impl DiskStorage {
    pub fn new(root: impl Into<PathBuf>) -> io::Result<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    fn path_for(&self, key: &str) -> io::Result<PathBuf> {
        let ok = !key.is_empty()
            && key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
            && !key.starts_with('.');
        if !ok {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "bad key"));
        }
        Ok(self.root.join(key))
    }
}

impl Storage for DiskStorage {
    fn put(&self, key: &str, bytes: &[u8]) -> io::Result<()> {
        let path = self.path_for(key)?;
        std::fs::write(path, bytes)
    }

    fn get(&self, key: &str) -> io::Result<Option<Vec<u8>>> {
        let path = self.path_for(key)?;
        match std::fs::read(&path) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn remove(&self, key: &str) -> io::Result<()> {
        let path = self.path_for(key)?;
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
}

/// Convenience: thumbnails are stored next to the original under `<id>.thumb`.
pub fn thumb_key(id: &str) -> String {
    format!("{id}.thumb")
}
