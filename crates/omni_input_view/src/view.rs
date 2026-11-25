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
    evidence::{Evidence, FileIdentity, Observation},
    limits::InputLimits,
    name::check_name,
};

const CHUNK: usize = 64 * 1024;

pub struct InputViewOptions {
    pub authorizer: Arc<dyn InputAuthorizer>,
    pub limits: InputLimits,
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

#[derive(Default)]
struct State {
    operations: usize,
    total_bytes: u64,
    total_dir_entries: usize,
    captured: HashMap<WorkspacePath, Arc<[u8]>>,
    evidence: Evidence,
}

pub struct InputView {
    root: ContainedRoot,
    authorizer: Arc<dyn InputAuthorizer>,
    limits: InputLimits,
    state: Mutex<State>,
}

impl InputView {
    /// Holds a handle on `root`. Where containment is unsupported this is
    /// `Containment(Unsupported)` and nothing is read.
    pub fn open(
        root: &Path,
        options: InputViewOptions,
    ) -> Result<Self, InputError> {
        Ok(Self {
            root: ContainedRoot::open(root)?,
            authorizer: options.authorizer,
            limits: options.limits,
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
        self.begin(path, cx)?;
        let Some((_file, metadata)) = self.probe(path)? else {
            self.record(path, Observation::Missing)?;
            return Ok(Exists::Missing);
        };
        let kind = kind_of(&metadata);
        let exists = match kind {
            EntryKind::File => Exists::File,
            EntryKind::Dir => Exists::Dir,
            other => return Err(InputError::NotRegular(other)),
        };
        self.record(path, Observation::Present(kind))?;
        Ok(exists)
    }

    pub fn list(
        &self,
        dir: &WorkspacePath,
        cx: &Cancellation,
    ) -> Result<List, InputError> {
        self.begin(dir, cx)?;
        let Some((probe, metadata)) = self.probe(dir)? else {
            self.record(dir, Observation::Missing)?;
            return Ok(List::Missing);
        };
        match kind_of(&metadata) {
            EntryKind::Dir => {}
            other => return Err(InputError::NotDirectory(other)),
        }
        let handle = if ContainedRoot::PROBE_FIRST {
            match self.root.open_path(dir, Want::Dir)? {
                Some(handle) => handle,
                None => {
                    // `ENOTDIR` also covers the path having become a file
                    // since the probe: look again before claiming it is
                    // absent.
                    return match self.probe(dir)? {
                        Some((_, now)) => {
                            Err(InputError::NotDirectory(kind_of(&now)))
                        }
                        None => {
                            self.record(dir, Observation::Missing)?;
                            Ok(List::Missing)
                        }
                    };
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
            self.charge_dir_entry()?;
            found.push((name, entry.kind));
        }
        found.sort_by(|a, b| a.0.cmp(&b.0));

        let mut digest = Blake3Digest::new();
        for (name, kind) in &found {
            digest.update(&(name.len() as u64).to_le_bytes());
            digest.update(name.as_bytes());
            digest.update(&[kind_byte(*kind)]);
        }
        let observation = Observation::Dir {
            entry_count: u32::try_from(found.len()).unwrap_or(u32::MAX),
            membership_digest: digest.finalize(),
            id: Some(id),
        };
        if cx.is_cancelled() {
            return Err(InputError::Cancelled);
        }
        self.record(dir, observation)?;
        Ok(List::Found(Listing { entries: found }))
    }

    /// `after_chunk` runs after every chunk read; tests cancel from it.
    pub(crate) fn read_with(
        &self,
        path: &WorkspacePath,
        cx: &Cancellation,
        after_chunk: &mut dyn FnMut(),
    ) -> Result<Read, InputError> {
        self.begin(path, cx)?;
        let captured = self.lock().captured.get(path).cloned();
        if let Some(bytes) = captured {
            return Ok(Read::Found(bytes));
        }
        let Some((probe_handle, metadata)) = self.probe(path)? else {
            self.record(path, Observation::Missing)?;
            return Ok(Read::Missing);
        };
        regular(&metadata)?;
        let file = if ContainedRoot::PROBE_FIRST {
            match self.root.open_path(path, Want::File)? {
                Some(file) => file,
                None => {
                    self.record(path, Observation::Missing)?;
                    return Ok(Read::Missing);
                }
            }
        } else {
            probe_handle
        };
        let metadata = handle_metadata(&file, path)?;
        regular(&metadata)?;
        let len = metadata.len();
        let limit = self.limits.max_file_bytes;
        if len > limit {
            return Err(InputError::TooLarge { len, limit });
        }
        // Charged for the attempt and not refunded if the read later fails.
        self.charge_bytes(len)?;
        if cx.is_cancelled() {
            return Err(InputError::Cancelled);
        }

        let (bytes, digest) =
            self.read_all(file, path, len, limit, cx, after_chunk)?;
        let bytes: Arc<[u8]> = bytes.into();
        let bytes = self.record_file(path, bytes, digest, &metadata)?;
        Ok(Read::Found(bytes))
    }

    fn read_all(
        &self,
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
                self.charge_bytes(total - charged)?;
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
        path: &WorkspacePath,
        cx: &Cancellation,
    ) -> Result<(), InputError> {
        {
            let mut state = self.lock();
            if state.operations >= self.limits.max_operations {
                return Err(InputError::Budget(BudgetKind::Operations));
            }
            state.operations += 1;
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

    fn charge_bytes(&self, bytes: u64) -> Result<(), InputError> {
        let mut state = self.lock();
        let total = state.total_bytes.saturating_add(bytes);
        if total > self.limits.max_total_bytes {
            return Err(InputError::Budget(BudgetKind::TotalBytes));
        }
        state.total_bytes = total;
        Ok(())
    }

    fn charge_dir_entry(&self) -> Result<(), InputError> {
        let mut state = self.lock();
        if state.total_dir_entries >= self.limits.max_total_dir_entries {
            return Err(InputError::Budget(BudgetKind::TotalDirEntries));
        }
        state.total_dir_entries += 1;
        Ok(())
    }

    /// Records the file observation and keeps the first capture: if another
    /// read of the same path finished first, its bytes win and are returned,
    /// so every caller sees one snapshot per path (a later divergence is a
    /// revalidation finding).
    fn record_file(
        &self,
        path: &WorkspacePath,
        bytes: Arc<[u8]>,
        digest: crate::digest::Digest,
        metadata: &Metadata,
    ) -> Result<Arc<[u8]>, InputError> {
        let mut state = self.lock();
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

    fn record(
        &self,
        path: &WorkspacePath,
        observation: Observation,
    ) -> Result<(), InputError> {
        let mut state = self.lock();
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
        let entry = state.evidence.paths.entry(path.clone()).or_default();
        if !entry.record(observation, self.limits.max_observations_per_path) {
            return Err(InputError::Budget(BudgetKind::Observations));
        }
        Ok(())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn regular(metadata: &Metadata) -> Result<(), InputError> {
    match kind_of(metadata) {
        EntryKind::File => Ok(()),
        other => Err(InputError::NotRegular(other)),
    }
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
