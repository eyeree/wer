//! Abstract persistence interface (sections 18 and 19 of the plan).
//!
//! The runtime never talks to the filesystem or IndexedDB directly. It stores
//! only *sparse deviations* from deterministic generation (named features,
//! preserves, modified terrain, ...) through this trait, which native and web
//! platforms implement over their respective backends. The interface is
//! key/value and byte-oriented so it maps onto both a file tree and a browser
//! object store. The current interface is synchronous; a successful mutation
//! means that the backend has crossed its documented durability boundary.

use core::fmt;

/// Errors a [`Storage`] backend may return.
#[derive(Debug)]
pub enum StorageError {
    /// The requested key does not exist.
    NotFound,
    /// A backend-specific failure, described for logging only.
    Backend(String),
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageError::NotFound => write!(f, "key not found"),
            StorageError::Backend(msg) => write!(f, "storage backend error: {msg}"),
        }
    }
}

impl core::error::Error for StorageError {}

/// A versioned, sparse key/value store for persistent world overrides.
///
/// Keys are opaque byte strings (callers namespace them, e.g. `disc/…`,
/// `route/…` — see `vault`). Implementations must be safe for partial loading
/// and must not assume the whole store fits in memory. Each successful
/// `store` call must be atomic and durable according to the backend: after a
/// crash the key holds either its old or its new value, never a torn write,
/// and the backend has completed the barrier required to retain that choice.
/// A successful `remove` likewise means that absence has crossed the
/// backend's durability boundary (ADR 0022).
pub trait Storage {
    /// Read the bytes stored at `key`, or [`StorageError::NotFound`].
    fn load(&self, key: &[u8]) -> Result<Vec<u8>, StorageError>;

    /// Write `value` at `key`, overwriting any existing entry. `Ok(())` is
    /// returned only after the backend's atomic durability boundary completes.
    fn store(&mut self, key: &[u8], value: &[u8]) -> Result<(), StorageError>;

    /// Remove `key`. Removing a missing key is not an error, but `Ok(())`
    /// still requires the backend's absence durability boundary to complete.
    fn remove(&mut self, key: &[u8]) -> Result<(), StorageError>;

    /// Every stored key that starts with `prefix`, in ascending byte order —
    /// how the vault enumerates a record namespace without an index record
    /// (phase-5-plan.md §5.2). Maps onto a directory listing natively and a
    /// key range in a browser object store.
    fn keys_with_prefix(&self, prefix: &[u8]) -> Result<Vec<Vec<u8>>, StorageError>;

    /// Whether `key` currently has a value.
    fn contains(&self, key: &[u8]) -> bool {
        self.load(key).is_ok()
    }
}

/// The in-memory reference [`Storage`]: a plain ordered map. Used by every
/// headless harness and test (platform-free by construction); the native
/// shell's file-tree implementation lives in `platform-native`, keeping the
/// neutral crates off the filesystem (ADR 0002).
#[derive(Debug, Default, Clone)]
pub struct MemoryStorage {
    entries: std::collections::BTreeMap<Vec<u8>, Vec<u8>>,
}

impl MemoryStorage {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of stored keys.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the store is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Total bytes held (keys + values) — the harness's sparsity gauge.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.entries.iter().map(|(k, v)| k.len() + v.len()).sum()
    }

    /// Iterate all entries in ascending key order.
    pub fn iter(&self) -> impl Iterator<Item = (&[u8], &[u8])> {
        self.entries
            .iter()
            .map(|(k, v)| (k.as_slice(), v.as_slice()))
    }
}

impl Storage for MemoryStorage {
    fn load(&self, key: &[u8]) -> Result<Vec<u8>, StorageError> {
        self.entries.get(key).cloned().ok_or(StorageError::NotFound)
    }

    fn store(&mut self, key: &[u8], value: &[u8]) -> Result<(), StorageError> {
        self.entries.insert(key.to_vec(), value.to_vec());
        Ok(())
    }

    fn remove(&mut self, key: &[u8]) -> Result<(), StorageError> {
        self.entries.remove(key);
        Ok(())
    }

    fn keys_with_prefix(&self, prefix: &[u8]) -> Result<Vec<Vec<u8>>, StorageError> {
        Ok(self
            .entries
            .range(prefix.to_vec()..)
            .take_while(|(k, _)| k.starts_with(prefix))
            .map(|(k, _)| k.clone())
            .collect())
    }

    fn contains(&self, key: &[u8]) -> bool {
        self.entries.contains_key(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_storage_honours_the_contract() {
        let mut s = MemoryStorage::new();
        assert!(matches!(s.load(b"a"), Err(StorageError::NotFound)));
        s.store(b"disc/2", b"two").unwrap();
        s.store(b"disc/1", b"one").unwrap();
        s.store(b"route/1", b"r").unwrap();
        assert_eq!(s.load(b"disc/1").unwrap(), b"one");
        assert!(s.contains(b"disc/2"));
        assert_eq!(
            s.keys_with_prefix(b"disc/").unwrap(),
            vec![b"disc/1".to_vec(), b"disc/2".to_vec()]
        );
        assert_eq!(s.keys_with_prefix(b"zzz/").unwrap(), Vec::<Vec<u8>>::new());
        s.remove(b"disc/1").unwrap();
        assert!(!s.contains(b"disc/1"));
        s.remove(b"disc/1").unwrap(); // removing a missing key is not an error
        assert_eq!(s.len(), 2);
        assert!(s.bytes() > 0);
    }
}
