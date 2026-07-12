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
//! file tree that mirrors the vault's key namespace one-to-one. A successful
//! write has durably created its ancestors, synchronized a complete
//! same-directory temp file, atomically renamed it, and synchronized the
//! containing directory. A successful removal synchronizes the containing (or
//! nearest existing) directory even when the key was already absent (ADR
//! 0022).
//!
//! Keys are validated against a conservative charset (the vault only emits
//! ASCII alphanumerics, `/`, `.`, `_`, `-`) and dot-prefixed names are
//! rejected/skipped, so temp files can never be mistaken for records and a
//! hostile key can never escape the root.

use std::collections::BTreeSet;
#[cfg(unix)]
use std::fs::File;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use world_runtime::{Storage, StorageError};

/// A [`Storage`] over a directory tree. See the module docs.
#[derive(Debug)]
pub struct FileStorage {
    root: PathBuf,
    ops: Box<dyn FileOps>,
    known_dirs: BTreeSet<PathBuf>,
    pending_dir_sync: BTreeSet<PathBuf>,
}

impl FileStorage {
    /// Open (creating if needed) a store rooted at `root`.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StorageError> {
        Self::open_with_ops(root.into(), Box::new(RealFileOps))
    }

    fn open_with_ops(root: PathBuf, mut ops: Box<dyn FileOps>) -> Result<Self, StorageError> {
        let mut known_dirs = BTreeSet::new();
        let mut pending_dir_sync = BTreeSet::new();
        let baseline = nearest_existing_dir(ops.as_ref(), &root)?;
        let baseline_parent = parent_or_dot(&baseline);
        if baseline_parent != baseline
            && (baseline == root || parent_or_dot(baseline_parent) != baseline_parent)
        {
            ops.sync_dir(baseline_parent).map_err(|error| {
                backend(
                    "sync nearest existing store-root ancestor parent",
                    baseline_parent,
                    &error,
                )
            })?;
        }
        known_dirs.insert(baseline);
        durable_create_dir_all(
            ops.as_mut(),
            &root,
            &mut known_dirs,
            &mut pending_dir_sync,
            false,
        )?;
        Ok(Self {
            root,
            ops,
            known_dirs,
            pending_dir_sync,
        })
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

static TEMP_SUFFIX: AtomicU64 = AtomicU64::new(0);
const TEMP_ATTEMPTS: usize = 128;

trait FileOps: core::fmt::Debug {
    fn is_dir(&self, path: &Path) -> bool;
    fn create_dir(&mut self, path: &Path) -> std::io::Result<()>;
    fn sync_dir(&mut self, path: &Path) -> std::io::Result<()>;
    fn create_new_file(&mut self, path: &Path) -> std::io::Result<()>;
    fn write_all(&mut self, path: &Path, value: &[u8]) -> std::io::Result<()>;
    fn sync_file(&mut self, path: &Path) -> std::io::Result<()>;
    fn rename(&mut self, from: &Path, to: &Path) -> std::io::Result<()>;
    fn remove_file(&mut self, path: &Path) -> std::io::Result<()>;
}

#[derive(Debug)]
struct RealFileOps;

impl FileOps for RealFileOps {
    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn create_dir(&mut self, path: &Path) -> std::io::Result<()> {
        fs::create_dir(path)
    }

    fn sync_dir(&mut self, path: &Path) -> std::io::Result<()> {
        sync_directory(path)
    }

    fn create_new_file(&mut self, path: &Path) -> std::io::Result<()> {
        OpenOptions::new().write(true).create_new(true).open(path)?;
        Ok(())
    }

    fn write_all(&mut self, path: &Path, value: &[u8]) -> std::io::Result<()> {
        let mut file = OpenOptions::new().write(true).open(path)?;
        file.write_all(value)
    }

    fn sync_file(&mut self, path: &Path) -> std::io::Result<()> {
        OpenOptions::new().write(true).open(path)?.sync_all()
    }

    fn rename(&mut self, from: &Path, to: &Path) -> std::io::Result<()> {
        fs::rename(from, to)
    }

    fn remove_file(&mut self, path: &Path) -> std::io::Result<()> {
        fs::remove_file(path)
    }
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> std::io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(windows)]
fn sync_directory(path: &Path) -> std::io::Result<()> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    OpenOptions::new()
        .write(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?
        .sync_all()
}

#[cfg(not(any(unix, windows)))]
fn sync_directory(_path: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        ErrorKind::Unsupported,
        "durable directory synchronization is unsupported on this platform",
    ))
}

fn parent_or_dot(path: &Path) -> &Path {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        Some(_) => Path::new("."),
        None if path.has_root() => path,
        None => Path::new("."),
    }
}

fn nearest_existing_dir(ops: &dyn FileOps, path: &Path) -> Result<PathBuf, StorageError> {
    let mut candidate = path.to_path_buf();
    loop {
        if ops.is_dir(&candidate) {
            return Ok(candidate);
        }
        let parent = parent_or_dot(&candidate);
        if parent == candidate {
            return Err(StorageError::Backend(format!(
                "no existing ancestor for store root {}",
                path.display()
            )));
        }
        candidate = parent.to_path_buf();
    }
}

/// Create every absent ancestor top-down and commit each new directory entry
/// by synchronizing its parent before proceeding.
fn durable_create_dir_all(
    ops: &mut dyn FileOps,
    path: &Path,
    known_dirs: &mut BTreeSet<PathBuf>,
    pending_dir_sync: &mut BTreeSet<PathBuf>,
    sync_unknown_existing: bool,
) -> Result<(), StorageError> {
    if known_dirs.contains(path) {
        if ops.is_dir(path) {
            return Ok(());
        }
        known_dirs.remove(path);
    }
    let parent = parent_or_dot(path);
    if parent != path {
        durable_create_dir_all(
            ops,
            parent,
            known_dirs,
            pending_dir_sync,
            sync_unknown_existing,
        )?;
    }
    if ops.is_dir(path) {
        if pending_dir_sync.contains(path) || (sync_unknown_existing && parent != path) {
            ops.sync_dir(parent)
                .map_err(|error| backend("retry directory parent sync", parent, &error))?;
            pending_dir_sync.remove(path);
        }
        // An existing baseline directory is trusted; only directories created
        // by this FileStorage instance carry a pending parent barrier.
        known_dirs.insert(path.to_path_buf());
        return Ok(());
    }
    match ops.create_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::AlreadyExists && ops.is_dir(path) => {}
        Err(error) => return Err(backend("create directory", path, &error)),
    }
    pending_dir_sync.insert(path.to_path_buf());
    ops.sync_dir(parent)
        .map_err(|error| backend("sync directory after create", parent, &error))?;
    pending_dir_sync.remove(path);
    known_dirs.insert(path.to_path_buf());
    Ok(())
}

fn sync_nearest_existing_dir(ops: &mut dyn FileOps, mut path: &Path) -> Result<(), StorageError> {
    while !ops.is_dir(path) {
        let parent = parent_or_dot(path);
        if parent == path {
            return Err(StorageError::Backend(format!(
                "no existing directory proves absence below {}",
                path.display()
            )));
        }
        path = parent;
    }
    ops.sync_dir(path)
        .map_err(|error| backend("sync directory after remove", path, &error))
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
        durable_create_dir_all(
            self.ops.as_mut(),
            dir,
            &mut self.known_dirs,
            &mut self.pending_dir_sync,
            true,
        )?;
        let file_name = path.file_name().expect("validated key").to_string_lossy();
        let mut temp = None;
        for _ in 0..TEMP_ATTEMPTS {
            let suffix = TEMP_SUFFIX.fetch_add(1, Ordering::Relaxed);
            let candidate = dir.join(format!(
                ".tmp-{file_name}-{}-{suffix:016x}",
                std::process::id()
            ));
            match self.ops.create_new_file(&candidate) {
                Ok(()) => {
                    temp = Some(candidate);
                    break;
                }
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(backend("create temp file", &candidate, &error)),
            }
        }
        let temp = temp.ok_or_else(|| {
            StorageError::Backend(format!(
                "create temp file in {}: exhausted {TEMP_ATTEMPTS} collision retries",
                dir.display()
            ))
        })?;

        let before_rename = (|| {
            self.ops
                .write_all(&temp, value)
                .map_err(|error| backend("write temp file", &temp, &error))?;
            self.ops
                .sync_file(&temp)
                .map_err(|error| backend("sync temp file", &temp, &error))?;
            self.ops
                .rename(&temp, &path)
                .map_err(|error| backend("atomic rename", &path, &error))?;
            Ok::<(), StorageError>(())
        })();
        if let Err(error) = before_rename {
            // The known temp exists only before a successful rename. Cleanup
            // is deliberately best effort and never replaces the staged error.
            let _ = self.ops.remove_file(&temp);
            return Err(error);
        }
        self.ops
            .sync_dir(dir)
            .map_err(|error| backend("sync directory after rename", dir, &error))
    }

    fn remove(&mut self, key: &[u8]) -> Result<(), StorageError> {
        let path = self.path_for(key)?;
        match self.ops.remove_file(&path) {
            Ok(()) => sync_nearest_existing_dir(
                self.ops.as_mut(),
                path.parent().expect("keys resolve under root"),
            ),
            Err(error) if error.kind() == ErrorKind::NotFound => sync_nearest_existing_dir(
                self.ops.as_mut(),
                path.parent().expect("keys resolve under root"),
            ),
            Err(error) => Err(backend("remove file", &path, &error)),
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
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    use std::rc::Rc;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum FakeCall {
        CreateDir(PathBuf),
        SyncDir(PathBuf),
        CreateTemp(PathBuf),
        Write(PathBuf, Vec<u8>),
        SyncFile(PathBuf),
        Rename(PathBuf, PathBuf),
        Remove(PathBuf),
    }

    #[derive(Debug, Default)]
    struct FakeState {
        dirs: BTreeSet<PathBuf>,
        files: BTreeSet<PathBuf>,
        calls: Vec<FakeCall>,
        fail_call: Option<usize>,
        collided: bool,
        collide_once: bool,
    }

    #[derive(Debug, Clone, Default)]
    struct FakeFileOps(Rc<RefCell<FakeState>>);

    impl FakeFileOps {
        fn with_dirs(dirs: impl IntoIterator<Item = PathBuf>) -> Self {
            let this = Self::default();
            this.0.borrow_mut().dirs.extend(dirs);
            this
        }

        fn calls(&self) -> Vec<FakeCall> {
            self.0.borrow().calls.clone()
        }

        fn clear_calls(&self) {
            self.0.borrow_mut().calls.clear();
        }

        fn fail_call(&self, index: usize) {
            self.0.borrow_mut().fail_call = Some(index);
        }

        fn should_fail(state: &mut FakeState) -> bool {
            let index = state.calls.len() - 1;
            if state.fail_call == Some(index) {
                state.fail_call = None;
                true
            } else {
                false
            }
        }

        fn failure() -> std::io::Error {
            std::io::Error::other("scripted file operation failure")
        }
    }

    impl FileOps for FakeFileOps {
        fn is_dir(&self, path: &Path) -> bool {
            self.0.borrow().dirs.contains(path)
        }

        fn create_dir(&mut self, path: &Path) -> std::io::Result<()> {
            let mut state = self.0.borrow_mut();
            state.calls.push(FakeCall::CreateDir(path.to_path_buf()));
            if Self::should_fail(&mut state) {
                return Err(Self::failure());
            }
            state.dirs.insert(path.to_path_buf());
            Ok(())
        }

        fn sync_dir(&mut self, path: &Path) -> std::io::Result<()> {
            let mut state = self.0.borrow_mut();
            state.calls.push(FakeCall::SyncDir(path.to_path_buf()));
            if Self::should_fail(&mut state) {
                return Err(Self::failure());
            }
            if !state.dirs.contains(path) {
                return Err(std::io::Error::new(
                    ErrorKind::NotFound,
                    "missing directory",
                ));
            }
            Ok(())
        }

        fn create_new_file(&mut self, path: &Path) -> std::io::Result<()> {
            let mut state = self.0.borrow_mut();
            state.calls.push(FakeCall::CreateTemp(path.to_path_buf()));
            if Self::should_fail(&mut state) {
                return Err(Self::failure());
            }
            if state.collide_once && !state.collided {
                state.collided = true;
                return Err(std::io::Error::new(ErrorKind::AlreadyExists, "collision"));
            }
            if !state.files.insert(path.to_path_buf()) {
                return Err(std::io::Error::new(ErrorKind::AlreadyExists, "collision"));
            }
            Ok(())
        }

        fn write_all(&mut self, path: &Path, value: &[u8]) -> std::io::Result<()> {
            let mut state = self.0.borrow_mut();
            state
                .calls
                .push(FakeCall::Write(path.to_path_buf(), value.to_vec()));
            if Self::should_fail(&mut state) {
                return Err(Self::failure());
            }
            Ok(())
        }

        fn sync_file(&mut self, path: &Path) -> std::io::Result<()> {
            let mut state = self.0.borrow_mut();
            state.calls.push(FakeCall::SyncFile(path.to_path_buf()));
            if Self::should_fail(&mut state) {
                return Err(Self::failure());
            }
            Ok(())
        }

        fn rename(&mut self, from: &Path, to: &Path) -> std::io::Result<()> {
            let mut state = self.0.borrow_mut();
            state
                .calls
                .push(FakeCall::Rename(from.to_path_buf(), to.to_path_buf()));
            if Self::should_fail(&mut state) {
                return Err(Self::failure());
            }
            if !state.files.remove(from) {
                return Err(std::io::Error::new(ErrorKind::NotFound, "missing temp"));
            }
            state.files.insert(to.to_path_buf());
            Ok(())
        }

        fn remove_file(&mut self, path: &Path) -> std::io::Result<()> {
            let mut state = self.0.borrow_mut();
            state.calls.push(FakeCall::Remove(path.to_path_buf()));
            if Self::should_fail(&mut state) {
                return Err(Self::failure());
            }
            if state.files.remove(path) {
                Ok(())
            } else {
                Err(std::io::Error::new(ErrorKind::NotFound, "missing file"))
            }
        }
    }

    fn fake_storage(root: &Path, namespace_exists: bool) -> (FileStorage, FakeFileOps) {
        let fake = FakeFileOps::with_dirs([PathBuf::from("/"), root.to_path_buf()]);
        if namespace_exists {
            fake.0.borrow_mut().dirs.insert(root.join("disc"));
        }
        let storage = FileStorage {
            root: root.to_path_buf(),
            ops: Box::new(fake.clone()),
            known_dirs: [root.to_path_buf()].into_iter().collect(),
            pending_dir_sync: BTreeSet::new(),
        };
        (storage, fake)
    }

    fn assert_store_tail(calls: &[FakeCall], dir: &Path, destination: &Path, value: &[u8]) {
        let [FakeCall::CreateTemp(temp), FakeCall::Write(written, bytes), FakeCall::SyncFile(synced), FakeCall::Rename(from, to), FakeCall::SyncDir(synced_dir)] =
            calls
        else {
            panic!("unexpected store protocol: {calls:?}");
        };
        assert_eq!(temp, written);
        assert_eq!(temp, synced);
        assert_eq!(temp, from);
        assert_eq!(bytes, value);
        assert_eq!(to, destination);
        assert_eq!(synced_dir, dir);
        assert!(
            temp.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".tmp-"),
            "temp is a hidden sibling"
        );
    }

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

    #[test]
    fn first_namespace_store_syncs_file_rename_and_both_directories_in_order() {
        let root = PathBuf::from("/vault");
        let (mut storage, fake) = fake_storage(&root, false);
        storage.store(b"disc/1", b"one").unwrap();
        let calls = fake.calls();
        assert_eq!(calls[0], FakeCall::CreateDir(root.join("disc")));
        assert_eq!(calls[1], FakeCall::SyncDir(root.clone()));
        assert_store_tail(
            &calls[2..],
            &root.join("disc"),
            &root.join("disc/1"),
            b"one",
        );
    }

    #[test]
    fn existing_namespace_store_runs_only_the_per_key_commit_protocol() {
        let root = PathBuf::from("/vault");
        let (mut storage, fake) = fake_storage(&root, true);
        storage.store(b"disc/1", b"one").unwrap();
        let calls = fake.calls();
        assert_eq!(calls[0], FakeCall::SyncDir(root.clone()));
        assert_store_tail(
            &calls[1..],
            &root.join("disc"),
            &root.join("disc/1"),
            b"one",
        );
    }

    #[test]
    fn every_store_stage_failure_stops_and_never_reports_success() {
        let root = PathBuf::from("/vault");
        for failed_call in 0..7 {
            let (mut storage, fake) = fake_storage(&root, false);
            fake.fail_call(failed_call);
            let error = storage.store(b"disc/1", b"one").unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("scripted file operation failure"),
                "stage {failed_call}: {error}"
            );
            let calls = fake.calls();
            assert!(calls.len() > failed_call);
            assert!(
                calls[..=failed_call].iter().all(
                    |call| !matches!(call, FakeCall::SyncDir(dir) if dir == &root.join("disc"))
                ) || failed_call == 6
            );
            if (3..=5).contains(&failed_call) {
                assert!(matches!(calls.last(), Some(FakeCall::Remove(_))));
            }
            if failed_call == 6 {
                assert!(
                    matches!(calls.last(), Some(FakeCall::SyncDir(dir)) if dir == &root.join("disc"))
                );
                assert!(fake.0.borrow().files.contains(&root.join("disc/1")));
            }
        }
    }

    #[test]
    fn retry_after_directory_creation_sync_failure_reissues_the_barrier() {
        let root = PathBuf::from("/vault");
        let (mut storage, fake) = fake_storage(&root, false);
        fake.fail_call(1);
        assert!(storage.store(b"disc/1", b"one").is_err());
        assert!(fake.0.borrow().dirs.contains(&root.join("disc")));

        fake.clear_calls();
        storage.store(b"disc/1", b"one").unwrap();
        let calls = fake.calls();
        assert_eq!(calls[0], FakeCall::SyncDir(root.clone()));
        assert_store_tail(
            &calls[1..],
            &root.join("disc"),
            &root.join("disc/1"),
            b"one",
        );
    }

    #[test]
    fn reopened_instance_reissues_unknown_namespace_parent_barrier() {
        let root = PathBuf::from("/vault");
        let (mut first, fake) = fake_storage(&root, false);
        fake.fail_call(1);
        assert!(first.store(b"disc/1", b"one").is_err());
        drop(first);
        fake.clear_calls();

        let mut reopened = FileStorage {
            root: root.clone(),
            ops: Box::new(fake.clone()),
            known_dirs: [root.clone()].into_iter().collect(),
            pending_dir_sync: BTreeSet::new(),
        };
        reopened.store(b"disc/1", b"one").unwrap();
        let calls = fake.calls();
        assert_eq!(calls[0], FakeCall::SyncDir(root.clone()));
        assert_store_tail(
            &calls[1..],
            &root.join("disc"),
            &root.join("disc/1"),
            b"one",
        );
    }

    #[test]
    fn temp_name_collision_retries_without_touching_the_colliding_file() {
        let root = PathBuf::from("/vault");
        let (mut storage, fake) = fake_storage(&root, true);
        fake.0.borrow_mut().collide_once = true;
        storage.store(b"disc/1", b"one").unwrap();
        let calls = fake.calls();
        assert_eq!(calls[0], FakeCall::SyncDir(root.clone()));
        let FakeCall::CreateTemp(first) = &calls[1] else {
            panic!("first call is temp creation");
        };
        let FakeCall::CreateTemp(second) = &calls[2] else {
            panic!("collision must retry temp creation");
        };
        assert_ne!(first, second);
        assert_store_tail(
            &calls[2..],
            &root.join("disc"),
            &root.join("disc/1"),
            b"one",
        );
    }

    #[test]
    fn post_rename_sync_failure_retries_the_whole_store_protocol() {
        let root = PathBuf::from("/vault");
        let (mut storage, fake) = fake_storage(&root, true);
        fake.fail_call(5);
        assert!(storage.store(b"disc/1", b"one").is_err());
        assert!(fake.0.borrow().files.contains(&root.join("disc/1")));

        fake.clear_calls();
        storage.store(b"disc/1", b"two").unwrap();
        assert_store_tail(
            &fake.calls(),
            &root.join("disc"),
            &root.join("disc/1"),
            b"two",
        );
    }

    #[test]
    fn remove_and_not_found_retry_both_cross_a_directory_barrier() {
        let root = PathBuf::from("/vault");
        let (mut storage, fake) = fake_storage(&root, true);
        fake.0.borrow_mut().files.insert(root.join("disc/1"));
        fake.fail_call(1);
        assert!(storage.remove(b"disc/1").is_err());
        assert_eq!(
            fake.calls(),
            vec![
                FakeCall::Remove(root.join("disc/1")),
                FakeCall::SyncDir(root.join("disc"))
            ]
        );
        assert!(!fake.0.borrow().files.contains(&root.join("disc/1")));

        fake.clear_calls();
        storage.remove(b"disc/1").unwrap();
        assert_eq!(
            fake.calls(),
            vec![
                FakeCall::Remove(root.join("disc/1")),
                FakeCall::SyncDir(root.join("disc"))
            ]
        );
    }

    #[test]
    fn remove_file_failure_stops_before_the_directory_barrier() {
        let root = PathBuf::from("/vault");
        let (mut storage, fake) = fake_storage(&root, true);
        fake.0.borrow_mut().files.insert(root.join("disc/1"));
        fake.fail_call(0);
        let error = storage.remove(b"disc/1").unwrap_err();
        assert!(error
            .to_string()
            .contains("scripted file operation failure"));
        assert_eq!(fake.calls(), vec![FakeCall::Remove(root.join("disc/1"))]);
        assert!(fake.0.borrow().files.contains(&root.join("disc/1")));
    }

    #[test]
    fn remove_from_never_created_namespace_syncs_nearest_existing_ancestor() {
        let root = PathBuf::from("/vault");
        let (mut storage, fake) = fake_storage(&root, false);
        storage.remove(b"disc/1").unwrap();
        assert_eq!(
            fake.calls(),
            vec![
                FakeCall::Remove(root.join("disc/1")),
                FakeCall::SyncDir(root)
            ]
        );
    }

    #[test]
    fn opening_nested_root_creates_and_syncs_each_ancestor_top_down() {
        let fake = FakeFileOps::with_dirs([PathBuf::from("/"), PathBuf::from("/base")]);
        let root = PathBuf::from("/base/one/two");
        let _storage = FileStorage::open_with_ops(root.clone(), Box::new(fake.clone())).unwrap();
        assert_eq!(
            fake.calls(),
            vec![
                FakeCall::CreateDir(PathBuf::from("/base/one")),
                FakeCall::SyncDir(PathBuf::from("/base")),
                FakeCall::CreateDir(root),
                FakeCall::SyncDir(PathBuf::from("/base/one")),
            ]
        );
    }

    #[test]
    fn opening_an_existing_root_does_not_require_ancestor_write_access() {
        let root = PathBuf::from("/base/existing");
        let fake =
            FakeFileOps::with_dirs([PathBuf::from("/"), PathBuf::from("/base"), root.clone()]);
        let _storage = FileStorage::open_with_ops(root, Box::new(fake.clone())).unwrap();
        assert_eq!(
            fake.calls(),
            vec![FakeCall::SyncDir(PathBuf::from("/base"))],
            "only the configured root's immediate parent needs a barrier"
        );
    }

    #[test]
    fn open_root_sync_failure_is_retried_when_creation_is_now_visible() {
        let root = PathBuf::from("/base/retry-root");
        let fake = FakeFileOps::with_dirs([PathBuf::from("/"), PathBuf::from("/base")]);
        fake.fail_call(1);
        assert!(FileStorage::open_with_ops(root.clone(), Box::new(fake.clone())).is_err());
        assert!(fake.0.borrow().dirs.contains(&root));

        fake.clear_calls();
        let _storage = FileStorage::open_with_ops(root, Box::new(fake.clone())).unwrap();
        assert_eq!(
            fake.calls(),
            vec![FakeCall::SyncDir(PathBuf::from("/base"))]
        );
    }

    #[test]
    fn relative_open_root_sync_failure_retries_the_current_directory_barrier() {
        let root = PathBuf::from("relative-retry-root");
        let fake = FakeFileOps::with_dirs([PathBuf::from(".")]);
        fake.fail_call(1);
        assert!(FileStorage::open_with_ops(root.clone(), Box::new(fake.clone())).is_err());
        assert!(fake.0.borrow().dirs.contains(&root));

        fake.clear_calls();
        let _storage = FileStorage::open_with_ops(root, Box::new(fake.clone())).unwrap();
        assert_eq!(fake.calls(), vec![FakeCall::SyncDir(PathBuf::from("."))]);
    }

    #[test]
    fn open_retries_a_failed_intermediate_parent_barrier() {
        let root = PathBuf::from("/base/retry-middle/leaf");
        let middle = PathBuf::from("/base/retry-middle");
        let fake = FakeFileOps::with_dirs([PathBuf::from("/"), PathBuf::from("/base")]);
        fake.fail_call(1);
        assert!(FileStorage::open_with_ops(root.clone(), Box::new(fake.clone())).is_err());
        assert!(fake.0.borrow().dirs.contains(&middle));
        assert!(!fake.0.borrow().dirs.contains(&root));

        fake.clear_calls();
        let _storage = FileStorage::open_with_ops(root.clone(), Box::new(fake.clone())).unwrap();
        assert_eq!(
            fake.calls(),
            vec![
                FakeCall::SyncDir(PathBuf::from("/base")),
                FakeCall::CreateDir(root),
                FakeCall::SyncDir(middle),
            ]
        );
    }

    #[test]
    fn durable_store_and_delete_survive_reopen_on_real_filesystem() {
        let root = temp_root("durable-reopen");
        {
            let mut storage = FileStorage::open(&root).unwrap();
            storage.store(b"disc/1", b"one").unwrap();
        }
        let mut reopened = FileStorage::open(&root).unwrap();
        assert_eq!(reopened.load(b"disc/1").unwrap(), b"one");
        reopened.remove(b"disc/1").unwrap();
        drop(reopened);
        let reopened = FileStorage::open(&root).unwrap();
        assert!(matches!(
            reopened.load(b"disc/1"),
            Err(StorageError::NotFound)
        ));
        let _ = fs::remove_dir_all(&root);
    }
}
