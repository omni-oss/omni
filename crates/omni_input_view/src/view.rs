//! The tracked view: authorize, open beneath the root, enforce limits before
//! allocating, and record what was observed. Effective authority is
//! `authorizer ∩ containment ∩ limits`; the view never grants.

use std::{
    collections::HashMap,
    fs::{File, Metadata},
    io::Read as _,
    path::Path,
    sync::{Arc, Mutex},
};

use omni_path_utils::{WorkspacePath, WorkspacePathError};

use crate::{
    auth::{AuthDecision, InputAuthorizer},
    cancel::Cancellation,
    contain::{ContainedRoot, Want, entries},
    digest::{Blake3Digest, DigestSink},
    error::{BudgetKind, ContainmentViolation, EntryKind, InputError},
    evidence::{Evidence, FileIdentity, Observation, PathEvidence},
    limits::InputLimits,
    name::check_name,
};

const CHUNK: usize = 64 * 1024;

pub struct InputViewOptions {
    pub authorizer: Arc<dyn InputAuthorizer>,
    pub limits: InputLimits,
    pub symlinks: SymlinkPolicy,
}

/// What to do with a symlink on a path. Only `Reject` is implemented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SymlinkPolicy {
    /// Every symlink or reparse point is a containment error (D2).
    #[default]
    Reject,
    /// Reserved for a later plan; selecting it is
    /// [`InputError::SymlinkPolicyUnsupported`] and nothing is followed.
    FollowInRoot { max_hops: u8 },
}

/// `Missing` is a tracked result, not an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Read {
    Found(Arc<[u8]>),
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exists {
    File,
    Dir,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum List {
    Found(Listing),
    Missing,
}

/// One directory level, sorted by name, authorized entries only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub entries: Vec<(String, EntryKind)>,
}

/// Counters for the budgets that charge per attempt. The live view has one;
/// every revalidation pass gets a fresh one, so a pass over everything that was
/// tracked cannot be starved by the reads that tracked it.
#[derive(Default)]
struct Meter {
    operations: usize,
    total_bytes: u64,
    total_dir_entries: usize,
}

#[derive(Default)]
struct State {
    captured: HashMap<WorkspacePath, Arc<[u8]>>,
    evidence: Evidence,
}

pub struct InputView {
    root: ContainedRoot,
    authorizer: Arc<dyn InputAuthorizer>,
    limits: InputLimits,
    live: Mutex<Meter>,
    state: Mutex<State>,
}

/// What a fresh look at a file found.
enum FileOutcome {
    Missing,
    NotFile(EntryKind),
    File(Box<FileCapture>),
}

struct FileCapture {
    bytes: Vec<u8>,
    digest: crate::digest::Digest,
    metadata: Metadata,
}

/// What a fresh look at a directory found.
enum DirOutcome {
    Missing,
    NotDir(EntryKind),
    Dir(DirCapture),
}

struct DirCapture {
    entries: Vec<(String, EntryKind)>,
    id: FileIdentity,
}

mod revalidate;
pub use revalidate::{Change, ChangeKind, InputsChanged, Revalidation};

impl InputView {
    /// Holds a handle on `root`. Where containment is unsupported this is
    /// `Containment(Unsupported)` and nothing is read.
    pub fn open(
        root: &Path,
        options: InputViewOptions,
    ) -> Result<Self, InputError> {
        // Checked before the root is touched: no resolver, hop counter or
        // re-authorization of resolved paths exists, so nothing is followed.
        if options.symlinks != SymlinkPolicy::Reject {
            return Err(InputError::SymlinkPolicyUnsupported);
        }
        Ok(Self {
            root: ContainedRoot::open(root)?,
            authorizer: options.authorizer,
            limits: options.limits,
            live: Mutex::default(),
            state: Mutex::default(),
        })
    }

    /// Everything observed so far, ordered by path.
    pub fn evidence(&self) -> Evidence {
        self.lock().evidence.clone()
    }

    pub fn read(
        &self,
        path: &WorkspacePath,
        cx: &Cancellation,
    ) -> Result<Read, InputError> {
        self.read_with(path, cx, &mut || {})
    }

    pub fn exists(
        &self,
        path: &WorkspacePath,
        cx: &Cancellation,
    ) -> Result<Exists, InputError> {
        self.begin(&self.live, path, cx)?;
        let Some(kind) = self.capture_kind(path)? else {
            self.record(cx, path, Observation::Missing)?;
            return Ok(Exists::Missing);
        };
        let exists = match kind {
            EntryKind::File => Exists::File,
            EntryKind::Dir => Exists::Dir,
            other => return Err(InputError::NotRegular(other)),
        };
        self.record(cx, path, Observation::Present(kind))?;
        Ok(exists)
    }

    pub fn list(
        &self,
        dir: &WorkspacePath,
        cx: &Cancellation,
    ) -> Result<List, InputError> {
        self.begin(&self.live, dir, cx)?;
        let capture = match self.capture_dir(&self.live, dir, cx)? {
            DirOutcome::Missing => {
                self.record(cx, dir, Observation::Missing)?;
                return Ok(List::Missing);
            }
            DirOutcome::NotDir(kind) => {
                return Err(InputError::NotDirectory(kind));
            }
            DirOutcome::Dir(capture) => capture,
        };
        let observation = Observation::Dir {
            entry_count: u32::try_from(capture.entries.len())
                .unwrap_or(u32::MAX),
            membership_digest: membership_digest(&capture.entries),
            id: Some(capture.id),
        };
        self.record(cx, dir, observation)?;
        Ok(List::Found(Listing {
            entries: capture.entries,
        }))
    }

    /// `after_chunk` runs after every chunk read; tests cancel from it.
    pub(crate) fn read_with(
        &self,
        path: &WorkspacePath,
        cx: &Cancellation,
        after_chunk: &mut dyn FnMut(),
    ) -> Result<Read, InputError> {
        self.begin(&self.live, path, cx)?;
        let captured = self.lock().captured.get(path).cloned();
        if let Some(bytes) = captured {
            if cx.is_cancelled() {
                return Err(InputError::Cancelled);
            }
            return Ok(Read::Found(bytes));
        }
        match self.capture_file(&self.live, path, cx, after_chunk)? {
            FileOutcome::Missing => {
                self.record(cx, path, Observation::Missing)?;
                Ok(Read::Missing)
            }
            FileOutcome::NotFile(kind) => Err(InputError::NotRegular(kind)),
            FileOutcome::File(capture) => {
                let bytes: Arc<[u8]> = capture.bytes.into();
                let bytes = self.record_file(
                    cx,
                    path,
                    bytes,
                    capture.digest,
                    &capture.metadata,
                )?;
                Ok(Read::Found(bytes))
            }
        }
    }

    /// The kind of what is at `path` right now, or `None` when absent.
    fn capture_kind(
        &self,
        path: &WorkspacePath,
    ) -> Result<Option<EntryKind>, InputError> {
        Ok(self.probe(path)?.map(|(_, metadata)| kind_of(&metadata)))
    }

    /// Opens and reads the file at `path` (already admitted by `begin`).
    fn capture_file(
        &self,
        meter: &Mutex<Meter>,
        path: &WorkspacePath,
        cx: &Cancellation,
        after_chunk: &mut dyn FnMut(),
    ) -> Result<FileOutcome, InputError> {
        let Some((probe_handle, metadata)) = self.probe(path)? else {
            return Ok(FileOutcome::Missing);
        };
        match kind_of(&metadata) {
            EntryKind::File => {}
            other => return Ok(FileOutcome::NotFile(other)),
        }
        let file = if ContainedRoot::PROBE_FIRST {
            match self.root.open_path(path, Want::File)? {
                Some(file) => file,
                None => return Ok(FileOutcome::Missing),
            }
        } else {
            probe_handle
        };
        let metadata = handle_metadata(&file, path)?;
        match kind_of(&metadata) {
            EntryKind::File => {}
            other => return Ok(FileOutcome::NotFile(other)),
        }
        let len = metadata.len();
        // Never more than a `Vec` can hold, even on a 32-bit target.
        let limit = self.limits.max_file_bytes.min(isize::MAX as u64);
        if len > limit {
            return Err(InputError::TooLarge { len, limit });
        }
        // Charged for the attempt and not refunded if the read later fails.
        self.charge_bytes(meter, len)?;
        if cx.is_cancelled() {
            return Err(InputError::Cancelled);
        }
        let (bytes, digest) =
            self.read_all(meter, file, path, len, limit, cx, after_chunk)?;
        Ok(FileOutcome::File(Box::new(FileCapture {
            bytes,
            digest,
            metadata,
        })))
    }

    /// Lists the directory at `dir` (already admitted by `begin`): every entry
    /// is authorized before it is counted, denied ones are omitted.
    fn capture_dir(
        &self,
        meter: &Mutex<Meter>,
        dir: &WorkspacePath,
        cx: &Cancellation,
    ) -> Result<DirOutcome, InputError> {
        let Some((probe, metadata)) = self.probe(dir)? else {
            return Ok(DirOutcome::Missing);
        };
        match kind_of(&metadata) {
            EntryKind::Dir => {}
            other => return Ok(DirOutcome::NotDir(other)),
        }
        let handle = if ContainedRoot::PROBE_FIRST {
            match self.root.open_path(dir, Want::Dir)? {
                Some(handle) => handle,
                None => {
                    // `ENOTDIR` also covers the path having become a file
                    // since the probe: look again before claiming it is
                    // absent.
                    return Ok(match self.probe(dir)? {
                        Some((_, now)) => DirOutcome::NotDir(kind_of(&now)),
                        None => DirOutcome::Missing,
                    });
                }
            }
        } else {
            probe
        };
        let id = identity(&handle_metadata(&handle, dir)?);

        let mut found: Vec<(String, EntryKind)> = Vec::new();
        let mut raw = 0usize;
        let reader = entries(handle).map_err(|error| io_error(dir, &error))?;
        for entry in reader {
            if cx.is_cancelled() {
                return Err(InputError::Cancelled);
            }
            let entry = entry.map_err(|error| io_error(dir, &error))?;
            raw += 1;
            if raw > self.limits.max_dir_scan_entries {
                return Err(InputError::Budget(BudgetKind::DirScan));
            }
            // A name that cannot be a workspace path at all (not UTF-8, a
            // backslash, a drive prefix) cannot be authorized, so it fails the
            // list. Every other entry is authorized first, so a denied entry
            // is omitted whatever its name, and only an allowed entry with a
            // name D9 rejects fails the list.
            let name = entry
                .name
                .map_err(|()| ContainmentViolation::NameUnsupported)?;
            let child = dir
                .join(&name)
                .map_err(|_| ContainmentViolation::NameUnsupported)?;
            match self.authorize(&child)? {
                AuthDecision::Deny(_) => continue,
                AuthDecision::Allow => {}
            }
            check_name(&name)?;
            if found.len() >= self.limits.max_dir_entries {
                return Err(InputError::Budget(BudgetKind::DirEntries));
            }
            self.charge_dir_entry(meter)?;
            found.push((name, entry.kind));
        }
        found.sort_by(|a, b| a.0.cmp(&b.0));
        if cx.is_cancelled() {
            return Err(InputError::Cancelled);
        }
        Ok(DirOutcome::Dir(DirCapture { entries: found, id }))
    }

    #[allow(clippy::too_many_arguments)]
    fn read_all(
        &self,
        meter: &Mutex<Meter>,
        file: File,
        path: &WorkspacePath,
        len: u64,
        limit: u64,
        cx: &Cancellation,
        after_chunk: &mut dyn FnMut(),
    ) -> Result<(Vec<u8>, crate::digest::Digest), InputError> {
        // `len <= limit` here, so this allocation is already bounded.
        let mut bytes = Vec::with_capacity(len as usize);
        let mut sink = Blake3Digest::new();
        // Already charged by the caller; growth beyond it is charged here,
        // before the bytes are buffered.
        let mut charged = len;
        let mut reader = file.take(limit.saturating_add(1));
        let mut chunk = vec![0u8; CHUNK.min(len.saturating_add(1) as usize)];
        loop {
            if cx.is_cancelled() {
                return Err(InputError::Cancelled);
            }
            let read = reader
                .read(&mut chunk)
                .map_err(|error| io_error(path, &error))?;
            if read == 0 {
                break;
            }
            // Refuse before growing the buffer past the limit.
            let total = bytes.len() as u64 + read as u64;
            if total > limit {
                return Err(InputError::TooLarge { len: total, limit });
            }
            if total > charged {
                self.charge_bytes(meter, total - charged)?;
                charged = total;
            }
            sink.update(&chunk[..read]);
            bytes.extend_from_slice(&chunk[..read]);
            after_chunk();
        }
        Ok((bytes, sink.finalize()))
    }

    /// Charges the attempt, then checks cancellation, path shape, names and
    /// authorization, in that order, before any filesystem access.
    fn begin(
        &self,
        meter: &Mutex<Meter>,
        path: &WorkspacePath,
        cx: &Cancellation,
    ) -> Result<(), InputError> {
        {
            let mut meter = lock(meter);
            if meter.operations >= self.limits.max_operations {
                return Err(InputError::Budget(BudgetKind::Operations));
            }
            meter.operations += 1;
        }
        if cx.is_cancelled() {
            return Err(InputError::Cancelled);
        }
        let len = path.as_str().len();
        if len > self.limits.max_path_bytes {
            return Err(InputError::InvalidPath(WorkspacePathError::TooLong {
                len,
                max: self.limits.max_path_bytes,
            }));
        }
        if path.segments().count() > self.limits.max_path_depth {
            return Err(InputError::Budget(BudgetKind::PathDepth));
        }
        path.segments().try_for_each(check_name)?;
        match self.authorize(path)? {
            // The authorizer may have cancelled while deciding.
            AuthDecision::Allow if cx.is_cancelled() => {
                Err(InputError::Cancelled)
            }
            AuthDecision::Allow => Ok(()),
            AuthDecision::Deny(reason) => Err(InputError::Denied(reason)),
        }
    }

    fn authorize(
        &self,
        path: &WorkspacePath,
    ) -> Result<AuthDecision, InputError> {
        self.authorizer
            .authorize_read(path)
            .map_err(|_| InputError::PolicyUnavailable)
    }

    /// A side-effect-free open that reports the kind (`O_PATH` on Linux). Off
    /// Linux the probe open is the real open and its handle is reused. It also
    /// accepts the workspace root, which `read` then refuses as a directory.
    fn probe(
        &self,
        path: &WorkspacePath,
    ) -> Result<Option<(File, Metadata)>, InputError> {
        let Some(file) = self.root.open_path(path, Want::Probe)? else {
            return Ok(None);
        };
        let metadata = handle_metadata(&file, path)?;
        Ok(Some((file, metadata)))
    }

    fn charge_bytes(
        &self,
        meter: &Mutex<Meter>,
        bytes: u64,
    ) -> Result<(), InputError> {
        let mut meter = lock(meter);
        let total = meter.total_bytes.saturating_add(bytes);
        if total > self.limits.max_total_bytes {
            return Err(InputError::Budget(BudgetKind::TotalBytes));
        }
        meter.total_bytes = total;
        Ok(())
    }

    fn charge_dir_entry(&self, meter: &Mutex<Meter>) -> Result<(), InputError> {
        let mut meter = lock(meter);
        if meter.total_dir_entries >= self.limits.max_total_dir_entries {
            return Err(InputError::Budget(BudgetKind::TotalDirEntries));
        }
        meter.total_dir_entries += 1;
        Ok(())
    }

    /// Records the file observation and keeps the first capture: if another
    /// read of the same path finished first, its bytes win and are returned,
    /// so every caller sees one snapshot per path (a later divergence is a
    /// revalidation finding). The losing read was already charged for its
    /// buffer, which over-counts the budget (the safe direction).
    fn record_file(
        &self,
        cx: &Cancellation,
        path: &WorkspacePath,
        bytes: Arc<[u8]>,
        digest: crate::digest::Digest,
        metadata: &Metadata,
    ) -> Result<Arc<[u8]>, InputError> {
        let mut state = self.lock();
        if cx.is_cancelled() {
            return Err(InputError::Cancelled);
        }
        if let Some(existing) = state.captured.get(path) {
            return Ok(existing.clone());
        }
        self.record_locked(
            &mut state,
            path,
            Observation::File {
                len: bytes.len() as u64,
                digest,
                id: Some(identity(metadata)),
            },
        )?;
        state.captured.insert(path.clone(), bytes.clone());
        Ok(bytes)
    }

    /// Evidence is only published for a call that was not cancelled, so a
    /// cancellation that arrives after admission still records nothing.
    fn record(
        &self,
        cx: &Cancellation,
        path: &WorkspacePath,
        observation: Observation,
    ) -> Result<(), InputError> {
        let mut state = self.lock();
        if cx.is_cancelled() {
            return Err(InputError::Cancelled);
        }
        self.record_locked(&mut state, path, observation)
    }

    fn record_locked(
        &self,
        state: &mut State,
        path: &WorkspacePath,
        observation: Observation,
    ) -> Result<(), InputError> {
        let tracked = state.evidence.paths.len();
        if !state.evidence.paths.contains_key(path)
            && tracked >= self.limits.max_tracked_inputs
        {
            return Err(InputError::Budget(BudgetKind::Inputs));
        }
        // A path is only added once its first observation is accepted, so a
        // refused observation leaves no empty entry behind.
        let max = self.limits.max_observations_per_path;
        if let Some(entry) = state.evidence.paths.get_mut(path) {
            if !entry.record(observation, max) {
                return Err(InputError::Budget(BudgetKind::Observations));
            }
        } else {
            let mut entry = PathEvidence::default();
            if !entry.record(observation, max) {
                return Err(InputError::Budget(BudgetKind::Observations));
            }
            state.evidence.paths.insert(path.clone(), entry);
        }
        Ok(())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        lock(&self.state)
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn membership_digest(entries: &[(String, EntryKind)]) -> crate::digest::Digest {
    let mut digest = Blake3Digest::new();
    for (name, kind) in entries {
        digest.update(&(name.len() as u64).to_le_bytes());
        digest.update(name.as_bytes());
        digest.update(&[kind_byte(*kind)]);
    }
    digest.finalize()
}

fn handle_metadata(
    file: &File,
    path: &WorkspacePath,
) -> Result<Metadata, InputError> {
    file.metadata().map_err(|error| io_error(path, &error))
}

fn io_error(path: &WorkspacePath, error: &std::io::Error) -> InputError {
    InputError::Io {
        path: path.clone(),
        kind: error.kind(),
    }
}

fn kind_of(metadata: &Metadata) -> EntryKind {
    let file_type = metadata.file_type();
    if file_type.is_file() {
        EntryKind::File
    } else if file_type.is_dir() {
        EntryKind::Dir
    } else if file_type.is_symlink() {
        EntryKind::Symlink
    } else {
        EntryKind::Other
    }
}

fn kind_byte(kind: EntryKind) -> u8 {
    match kind {
        EntryKind::File => 0,
        EntryKind::Dir => 1,
        EntryKind::Symlink => 2,
        EntryKind::Other => 3,
    }
}

#[cfg(unix)]
fn identity(metadata: &Metadata) -> FileIdentity {
    use std::os::unix::fs::MetadataExt;
    let nanos = |secs: i64, nsec: i64| {
        Some(i128::from(secs) * 1_000_000_000 + i128::from(nsec))
    };
    FileIdentity {
        dev: Some(metadata.dev()),
        ino: Some(metadata.ino()),
        size: metadata.len(),
        mtime_ns: nanos(metadata.mtime(), metadata.mtime_nsec()),
        ctime_ns: nanos(metadata.ctime(), metadata.ctime_nsec()),
    }
}

#[cfg(not(unix))]
fn identity(metadata: &Metadata) -> FileIdentity {
    let mtime_ns = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos() as i128);
    FileIdentity {
        dev: None,
        ino: None,
        size: metadata.len(),
        mtime_ns,
        ctime_ns: None,
    }
}

#[cfg(all(test, any(target_os = "linux", windows)))]
mod tests;

#[cfg(all(test, not(any(target_os = "linux", windows))))]
mod unsupported_tests {
    use super::*;

    struct Never;

    impl InputAuthorizer for Never {
        fn authorize_read(
            &self,
            _: &WorkspacePath,
        ) -> Result<AuthDecision, crate::auth::AuthError> {
            Err(crate::auth::AuthError)
        }
    }

    fn options(symlinks: SymlinkPolicy) -> InputViewOptions {
        InputViewOptions {
            authorizer: Arc::new(Never),
            limits: InputLimits::default(),
            symlinks,
        }
    }

    #[test]
    fn open_is_unsupported_without_touching_the_filesystem() {
        let root = Path::new("/does/not/exist");
        assert!(matches!(
            InputView::open(root, options(SymlinkPolicy::Reject)),
            Err(InputError::Containment(ContainmentViolation::Unsupported))
        ));
        assert!(matches!(
            InputView::open(
                root,
                options(SymlinkPolicy::FollowInRoot { max_hops: 1 })
            ),
            Err(InputError::SymlinkPolicyUnsupported)
        ));
    }
}
