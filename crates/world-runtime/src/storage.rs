//! Abstract persistence interface (sections 18 and 19 of the plan).
//!
//! The runtime never talks to the filesystem or IndexedDB directly. It stores
//! only *sparse deviations* from deterministic generation (named features,
//! preserves, modified terrain, ...) through this trait, which native and web
//! platforms implement over their respective backends. The interface is
//! key/value and byte-oriented so it maps onto both a file tree and a browser
//! object store, and callers must treat it as potentially asynchronous.

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
/// Keys are opaque byte strings (callers namespace them, e.g. `region/…`,
/// `route/…`). Implementations must be safe for partial loading and must not
/// assume the whole store fits in memory.
pub trait Storage {
    /// Read the bytes stored at `key`, or [`StorageError::NotFound`].
    fn load(&self, key: &[u8]) -> Result<Vec<u8>, StorageError>;

    /// Write `value` at `key`, overwriting any existing entry.
    fn store(&mut self, key: &[u8], value: &[u8]) -> Result<(), StorageError>;

    /// Remove `key`. Removing a missing key is not an error.
    fn remove(&mut self, key: &[u8]) -> Result<(), StorageError>;

    /// Whether `key` currently has a value.
    fn contains(&self, key: &[u8]) -> bool {
        matches!(self.load(key), Ok(_))
    }
}
