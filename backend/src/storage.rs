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

    /// Byte length of `key`, or None if absent. Default reads the whole blob; the
    /// disk backend overrides it with a cheap `metadata` call.
    fn size(&self, key: &str) -> io::Result<Option<u64>> {
        Ok(self.get(key)?.map(|v| v.len() as u64))
    }

    /// `len` bytes starting at `start` (clamped to the blob) for HTTP range /
    /// video seeking. Default reads the whole blob and slices; the disk backend
    /// overrides it with a seek + partial read.
    fn read_range(&self, key: &str, start: u64, len: u64) -> io::Result<Option<Vec<u8>>> {
        Ok(self.get(key)?.map(|v| {
            let s = (start as usize).min(v.len());
            let e = s.saturating_add(len as usize).min(v.len());
            v[s..e].to_vec()
        }))
    }
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

    fn size(&self, key: &str) -> io::Result<Option<u64>> {
        match std::fs::metadata(self.path_for(key)?) {
            Ok(m) => Ok(Some(m.len())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn read_range(&self, key: &str, start: u64, len: u64) -> io::Result<Option<Vec<u8>>> {
        use std::io::{Read, Seek, SeekFrom};
        let mut f = match std::fs::File::open(self.path_for(key)?) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        f.seek(SeekFrom::Start(start))?;
        let mut buf = vec![0u8; len as usize];
        let mut read = 0;
        while read < buf.len() {
            match f.read(&mut buf[read..])? {
                0 => break,
                n => read += n,
            }
        }
        buf.truncate(read);
        Ok(Some(buf))
    }
}

/// Convenience: thumbnails are stored next to the original under `<id>.thumb`.
pub fn thumb_key(id: &str) -> String {
    format!("{id}.thumb")
}
