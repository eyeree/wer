//! Native file-tree [`Storage`] backend (phase-5-plan.md §5.3).
//!
//! Lives in `tools` so the native shell (`wer`) and the atlas/vault tools
//! (`wer-atlas`, `wer-vault`) share one implementation without the tools
//! pulling in windowing/GPU dependencies. The crate-boundary rule is intact:
//! the *neutral* crates never touch the filesystem — this is platform-side
//! code behind the `Storage` trait (ADR 0002).
//!
//! Each key maps onto a relative path under the store's root directory
//! (`disc/00ab…` → `<root>/disc/00ab…`), so the store is a human-inspectable
//! file tree that mirrors the vault's key namespace one-to-one. Writes are
//! atomic per key — write to a dot-prefixed temp sibling, then `rename` into
//! place — satisfying the `Storage` crash-consistency contract (§7.7): after a
//! crash a key holds either its old or its new whole value, never a torn one.
//!
//! Keys are validated against a conservative charset (the vault only emits
//! ASCII alphanumerics, `/`, `.`, `_`, `-`) and dot-prefixed names are
//! rejected/skipped, so temp files can never be mistaken for records and a
//! hostile key can never escape the root.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use world_runtime::{Storage, StorageError};

/// A [`Storage`] over a directory tree. See the module docs.
#[derive(Debug)]
pub struct FileStorage {
    root: PathBuf,
}

impl FileStorage {
    /// Open (creating if needed) a store rooted at `root`.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StorageError> {
        let root = root.into();
        fs::create_dir_all(&root).map_err(|e| backend("create store root", &root, &e))?;
        Ok(Self { root })
    }

    /// Resolve a key to its path, refusing anything outside the safe charset.
    fn path_for(&self, key: &[u8]) -> Result<PathBuf, StorageError> {
        let key = valid_key(key).ok_or_else(|| {
            StorageError::Backend(format!(
                "invalid storage key {:?}",
                String::from_utf8_lossy(key)
            ))
        })?;
        Ok(self.root.join(key))
    }
}

/// Validate a key: non-empty `/`-separated segments of
/// `[A-Za-z0-9._-]`, none empty, none dot-prefixed (reserves dot names for
/// temp files and rules out `.`/`..` traversal).
fn valid_key(key: &[u8]) -> Option<&str> {
    let s = core::str::from_utf8(key).ok()?;
    if s.is_empty() {
        return None;
    }
    for segment in s.split('/') {
        if segment.is_empty() || segment.starts_with('.') {
            return None;
        }
        if !segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
        {
            return None;
        }
    }
    Some(s)
}

fn backend(what: &str, path: &Path, err: &std::io::Error) -> StorageError {
    StorageError::Backend(format!("{what} {}: {err}", path.display()))
}

impl Storage for FileStorage {
    fn load(&self, key: &[u8]) -> Result<Vec<u8>, StorageError> {
        let path = self.path_for(key)?;
        match fs::read(&path) {
            Ok(bytes) => Ok(bytes),
            Err(e) if e.kind() == ErrorKind::NotFound => Err(StorageError::NotFound),
            Err(e) => Err(backend("read", &path, &e)),
        }
    }

    fn store(&mut self, key: &[u8], value: &[u8]) -> Result<(), StorageError> {
        let path = self.path_for(key)?;
        let dir = path.parent().expect("keys resolve under the root");
        fs::create_dir_all(dir).map_err(|e| backend("create", dir, &e))?;
        let file_name = path.file_name().expect("validated key").to_string_lossy();
        let tmp = dir.join(format!(".tmp-{file_name}"));
        fs::write(&tmp, value).map_err(|e| backend("write", &tmp, &e))?;
        fs::rename(&tmp, &path).map_err(|e| backend("rename", &path, &e))
    }

    fn remove(&mut self, key: &[u8]) -> Result<(), StorageError> {
        let path = self.path_for(key)?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            Err(e) => Err(backend("remove", &path, &e)),
        }
    }

    fn keys_with_prefix(&self, prefix: &[u8]) -> Result<Vec<Vec<u8>>, StorageError> {
        let prefix = core::str::from_utf8(prefix)
            .map_err(|_| StorageError::Backend("non-utf8 key prefix".into()))?;
        let mut keys = Vec::new();
        walk(&self.root, String::new(), &mut keys)?;
        keys.retain(|k| k.starts_with(prefix.as_bytes()));
        keys.sort_unstable();
        Ok(keys)
    }
}

/// Collect every record key under `dir` (relative key prefix `rel`), skipping
/// dot-prefixed entries (temp files) and anything that fails key validation.
fn walk(dir: &Path, rel: String, keys: &mut Vec<Vec<u8>>) -> Result<(), StorageError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(backend("list", dir, &e)),
    };
    for entry in entries {
        let entry = entry.map_err(|e| backend("list", dir, &e))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue; // never produced by the vault
        };
        if name.starts_with('.') {
            continue; // temp file or hidden
        }
        let child_rel = if rel.is_empty() {
            name.to_string()
        } else {
            format!("{rel}/{name}")
        };
        let path = entry.path();
        if path.is_dir() {
            walk(&path, child_rel, keys)?;
        } else if valid_key(child_rel.as_bytes()).is_some() {
            keys.push(child_rel.into_bytes());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("wer-storage-test-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn file_storage_honours_the_contract() {
        let root = temp_root("contract");
        let mut s = FileStorage::open(&root).unwrap();
        assert!(matches!(s.load(b"meta/store"), Err(StorageError::NotFound)));
        s.store(b"disc/2", b"two").unwrap();
        s.store(b"disc/1", b"one").unwrap();
        s.store(b"route/1", b"r").unwrap();
        s.store(b"meta/store", b"m").unwrap();
        assert_eq!(s.load(b"disc/1").unwrap(), b"one");
        s.store(b"disc/1", b"uno").unwrap(); // overwrite in place
        assert_eq!(s.load(b"disc/1").unwrap(), b"uno");
        assert_eq!(
            s.keys_with_prefix(b"disc/").unwrap(),
            vec![b"disc/1".to_vec(), b"disc/2".to_vec()]
        );
        assert_eq!(s.keys_with_prefix(b"zzz/").unwrap(), Vec::<Vec<u8>>::new());
        s.remove(b"disc/1").unwrap();
        s.remove(b"disc/1").unwrap(); // missing key is not an error
        assert!(!s.contains(b"disc/1"));
        assert!(s.contains(b"meta/store"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn hostile_keys_are_refused() {
        let root = temp_root("hostile");
        let mut s = FileStorage::open(&root).unwrap();
        for key in [
            b"../escape".as_slice(),
            b"a/../../b",
            b"/rooted",
            b"a//b",
            b".hidden",
            b"a/.tmp-x",
            b"nul\0byte",
            b"",
        ] {
            assert!(
                s.store(key, b"x").is_err(),
                "key {:?} must be refused",
                String::from_utf8_lossy(key)
            );
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn temp_files_are_invisible_to_listing() {
        let root = temp_root("tmpfiles");
        let mut s = FileStorage::open(&root).unwrap();
        s.store(b"disc/1", b"one").unwrap();
        // A leftover temp file from a crash mid-write.
        fs::write(root.join("disc").join(".tmp-2"), b"torn").unwrap();
        assert_eq!(
            s.keys_with_prefix(b"disc/").unwrap(),
            vec![b"disc/1".to_vec()]
        );
        let _ = fs::remove_dir_all(&root);
    }
}
