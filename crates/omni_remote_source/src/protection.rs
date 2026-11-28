//! Tolerant reader for workspace registrations and their reference sets.
//!
//! A store may be shared by many workspaces. Each workspace registers itself in
//! `<store>/workspaces/<hash>.json`, pointing at its lockfile and so at its
//! `refs/` directory. Prune reads those live: a registration holds no checkout
//! list, so it cannot go stale (RFC 0024 F08.7).
//!
//! Reference files here are written by other omni versions and by every
//! subsystem, so this reader is deliberately tolerant: unknown kinds are
//! ignored, and a file that cannot be read or parsed fails the consulting prune
//! closed instead of silently dropping protection. Reads retry transient I/O,
//! time out, and never wait on the workspace advisory lock.

use std::{
    collections::HashSet,
    future::Future,
    io,
    path::{Path, PathBuf},
    time::Duration,
};

use crate::{
    driver::{
        Connection, DriverRegistry, SourceSpec, is_safe_segment, safe_slug,
    },
    error::Error,
    refs::RefSetData,
    registration::{REGISTRATIONS_SEGMENT, RegistrationData},
    sys::RemoteSourceSys,
};

/// `(kind, store slug, pin)`: the identity of one checkout, matching
/// [`StoreEntry`](crate::manager::StoreEntry)'s `(kind, remote, pin)`.
pub(crate) type ReferenceKey = (String, String, String);

/// How many times a read is attempted and how long one attempt may take. The
/// timeout is a policy value so unit tests can shorten it; production uses
/// [`Default`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct ReferenceReadPolicy {
    pub timeout: Duration,
    pub attempts: u32,
}

impl Default for ReferenceReadPolicy {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(5),
            attempts: 3,
        }
    }
}

/// A failed read of the registration/refs tree. `timed_out` decides whether the
/// run stops re-reading other entries after this error.
#[derive(Debug, Clone)]
pub(crate) struct ReadError {
    message: String,
    timed_out: bool,
}

impl ReadError {
    fn fatal(message: String) -> Self {
        Self {
            message,
            timed_out: false,
        }
    }

    fn timeout(after: Duration) -> Self {
        Self {
            message: format!("timed out after {after:?}"),
            timed_out: true,
        }
    }

    pub(crate) fn message(&self) -> &str {
        &self.message
    }

    pub(crate) fn is_timeout(&self) -> bool {
        self.timed_out
    }

    pub(crate) fn into_error(self) -> Error {
        Error::custom(self.message)
    }
}

/// The filesystem operations the reader needs. It exists so the retry, timeout
/// and stop rules can be unit tested with a scripted implementation instead of
/// a hung real filesystem. [`RemoteSourceSys`] gets the production impl below.
pub(crate) trait ReaderFs {
    async fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>>;
    /// `Ok` when the path exists, `NotFound` when it does not; other errors are
    /// uncertainty.
    async fn probe(&self, path: &Path) -> io::Result<()>;
    async fn read_file(&self, path: &Path) -> io::Result<Vec<u8>>;
}

impl<T: RemoteSourceSys> ReaderFs for T {
    async fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        self.fs_read_dir_async(path).await
    }

    async fn probe(&self, path: &Path) -> io::Result<()> {
        self.fs_metadata_async(path).await.map(|_| ())
    }

    async fn read_file(&self, path: &Path) -> io::Result<Vec<u8>> {
        Ok(self.fs_read_async(path).await?.into_owned())
    }
}

/// The checkouts one registration/liveness sweep protects.
#[derive(Debug, Default)]
pub(crate) struct ProtectedReferences(HashSet<ReferenceKey>);

impl ProtectedReferences {
    /// Build the union of every live registration's references and the
    /// consulting workspace's own `refs/` directory.
    pub(crate) async fn collect<TSys, F>(
        fs: &F,
        registry: &DriverRegistry<TSys>,
        store_root: &Path,
        own_refs_dir: &Path,
        policy: ReferenceReadPolicy,
    ) -> Result<Self, ReadError>
    where
        TSys: RemoteSourceSys,
        F: ReaderFs,
    {
        let mut keys = HashSet::new();
        for refs_dir in registration_refs_dirs(fs, store_root, policy).await? {
            read_reference_dir(fs, registry, &refs_dir, policy, &mut keys)
                .await?;
        }
        read_reference_dir(fs, registry, own_refs_dir, policy, &mut keys)
            .await?;
        Ok(Self(keys))
    }

    pub(crate) fn contains(&self, key: &ReferenceKey) -> bool {
        self.0.contains(key)
    }
}

/// Why a selected entry is (or is not) retained by references.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Recheck {
    /// Not referenced: the caller may delete it.
    Clear,
    /// A live registration or the workspace's own refs name this checkout.
    Referenced,
    /// The registry could not be read; the entry must not be deleted.
    Failed(String),
}

/// State shared by the entries of one prune call. A timeout stops further
/// re-reads so a hung mount cannot stall every entry. It is a local in
/// `prune_store_report`, never stored on the manager, so a dry run cannot
/// poison the confirmed call that follows it.
#[derive(Debug, Default)]
pub(crate) struct RegistryState {
    stopped: Option<String>,
}

impl RegistryState {
    pub(crate) fn stopped(&self) -> Option<&str> {
        self.stopped.as_deref()
    }

    fn stop(&mut self, cause: &str) {
        if self.stopped.is_none() {
            self.stopped = Some(cause.to_string());
        }
    }
}

/// Decide whether one entry is referenced. The caller has already taken the
/// exclusive lease (or returned the entry as in use) before calling this, so a
/// stop from an earlier timeout is checked first, then a fresh read of
/// registrations and the workspace's own refs.
pub(crate) async fn recheck<TSys, F>(
    fs: &F,
    registry: &DriverRegistry<TSys>,
    store_root: &Path,
    own_refs_dir: &Path,
    policy: ReferenceReadPolicy,
    state: &mut RegistryState,
    key: &ReferenceKey,
) -> Recheck
where
    TSys: RemoteSourceSys,
    F: ReaderFs,
{
    if let Some(cause) = state.stopped() {
        return Recheck::Failed(cause.to_string());
    }
    match ProtectedReferences::collect(
        fs,
        registry,
        store_root,
        own_refs_dir,
        policy,
    )
    .await
    {
        Ok(protected) if protected.contains(key) => Recheck::Referenced,
        Ok(_) => Recheck::Clear,
        Err(error) => {
            if error.is_timeout() {
                state.stop(error.message());
            }
            Recheck::Failed(error.message().to_string())
        }
    }
}

/// Read one reference directory, ignoring files that are not `*.json`.
async fn read_reference_dir<TSys, F>(
    fs: &F,
    registry: &DriverRegistry<TSys>,
    dir: &Path,
    policy: ReferenceReadPolicy,
    keys: &mut HashSet<ReferenceKey>,
) -> Result<(), ReadError>
where
    TSys: RemoteSourceSys,
    F: ReaderFs,
{
    let entries = retry(policy, || async {
        match fs.read_dir(dir).await {
            Ok(entries) => Ok(entries),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Ok(Vec::new())
            }
            Err(error) => Err(error),
        }
    })
    .await
    .map_err(|error| error.with_path(dir))?;
    for entry in entries {
        if entry.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let bytes = retry(policy, || fs.read_file(&entry))
            .await
            .map_err(|error| error.with_path(&entry))?;
        let data: RefSetData = omni_file_data_serde::from_slice(
            &bytes,
            omni_file_data_serde::Format::Json,
        )
        .map_err(|error| {
            ReadError::fatal(format!(
                "cannot read the reference set {}: {error}",
                entry.display()
            ))
        })?;
        insert_reference_keys(registry, &entry, &data, keys)?;
    }
    Ok(())
}

/// Map foreign references to store checkouts through the pruner's own drivers.
/// Unknown kinds are ignored before any pin check: the store only lists
/// registered kinds, so they cannot name a checkout this omni owns.
fn insert_reference_keys<TSys>(
    registry: &DriverRegistry<TSys>,
    file: &Path,
    data: &RefSetData,
    keys: &mut HashSet<ReferenceKey>,
) -> Result<(), ReadError>
where
    TSys: RemoteSourceSys,
{
    let RefSetData::V1_0_0(data) = data;
    for (kind, locators) in &data.sources {
        let Some(driver) = registry.get(kind) else {
            continue;
        };
        for (locator, selectors) in locators {
            for (selector, value) in selectors {
                if !is_safe_segment(&value.pin) {
                    return Err(ReadError::fatal(format!(
                        "{} contains an unsafe reference pin",
                        file.display()
                    )));
                }
                let spec = SourceSpec {
                    locator: locator.clone(),
                    selector: selector.clone(),
                    // The connection carries credentials and is never stored in
                    // a reference. A driver's slug must be a pure function of
                    // the locator and selector.
                    connection: Connection::new(locator.clone()),
                };
                keys.insert((
                    kind.clone(),
                    safe_slug(&driver.slug(&spec)),
                    value.pin.clone(),
                ));
            }
        }
    }
    Ok(())
}

/// Every live registration's `refs/` directory. A registration whose directory
/// is `NotFound` is not live for this run and is left on disk; any other probe
/// error is retried and then fails closed.
async fn registration_refs_dirs<F>(
    fs: &F,
    store_root: &Path,
    policy: ReferenceReadPolicy,
) -> Result<Vec<PathBuf>, ReadError>
where
    F: ReaderFs,
{
    let dir = store_root.join(REGISTRATIONS_SEGMENT);
    let entries = retry(policy, || async {
        match fs.read_dir(&dir).await {
            Ok(entries) => Ok(entries),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Ok(Vec::new())
            }
            Err(error) => Err(error),
        }
    })
    .await
    .map_err(|error| error.with_path(&dir))?;
    let mut refs_dirs = Vec::new();
    for entry in entries {
        if entry.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let bytes = retry(policy, || fs.read_file(&entry))
            .await
            .map_err(|error| error.with_path(&entry))?;
        let data = RegistrationData::from_file_bytes(&entry, &bytes)
            .map_err(|error| ReadError::fatal(error.to_string()))?;
        let lockfile = Path::new(data.lockfile_path());
        if !lockfile.is_absolute() {
            return Err(ReadError::fatal(format!(
                "the workspace registration {} names a non-absolute lockfile path {}",
                entry.display(),
                data.lockfile_path()
            )));
        }
        let Some(parent) = lockfile.parent() else {
            return Err(ReadError::fatal(format!(
                "the workspace registration {} names a lockfile with no parent",
                entry.display()
            )));
        };
        let refs_dir = parent.join(omni_constants::REFS_SEGMENT);
        if refs_dir_is_live(fs, &refs_dir, policy).await? {
            refs_dirs.push(refs_dir);
        }
    }
    Ok(refs_dirs)
}

/// One filesystem value, retried on transient I/O. A timeout is not retried.
async fn retry<T, F, Fut>(
    policy: ReferenceReadPolicy,
    mut op: F,
) -> Result<T, ReadError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = io::Result<T>>,
{
    let mut last = None;
    for attempt in 0..policy.attempts.max(1) {
        match tokio::time::timeout(policy.timeout, op()).await {
            Ok(Ok(value)) => return Ok(value),
            Ok(Err(error)) => last = Some(error),
            Err(_) => return Err(ReadError::timeout(policy.timeout)),
        }
        if attempt + 1 < policy.attempts.max(1) {
            let delay =
                Duration::from_millis(if attempt == 0 { 10 } else { 25 });
            tokio::time::sleep(delay).await;
        }
    }
    Err(ReadError::fatal(last.map_or_else(
        || "the read was not attempted".to_string(),
        |error| error.to_string(),
    )))
}

/// `Ok(true)` if the directory exists, `Ok(false)` for `NotFound` (a value,
/// never retried), and a retried failure for any other error.
async fn refs_dir_is_live<F>(
    fs: &F,
    dir: &Path,
    policy: ReferenceReadPolicy,
) -> Result<bool, ReadError>
where
    F: ReaderFs,
{
    retry(policy, || async {
        match fs.probe(dir).await {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    })
    .await
    .map_err(|error| error.with_path(dir))
}

impl ReadError {
    /// Give a read error the path it failed on when the operation did not.
    fn with_path(mut self, path: &Path) -> Self {
        let path = path.display().to_string();
        if !self.message.contains(&path) {
            self.message = format!("{}: {path}", self.message);
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        sync::{
            Mutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use super::*;
    use crate::driver::DriverRegistry;
    use system_traits::impls::RealSys;

    fn registry() -> DriverRegistry<RealSys> {
        DriverRegistry::builtin()
    }

    const SHORT: ReferenceReadPolicy = ReferenceReadPolicy {
        timeout: Duration::from_millis(20),
        attempts: 3,
    };

    #[derive(Default)]
    struct ScriptedFs {
        reads: Mutex<VecDeque<io::Result<Vec<u8>>>>,
        listings: Mutex<VecDeque<io::Result<Vec<PathBuf>>>>,
        probes: Mutex<VecDeque<io::Result<()>>>,
        read_calls: AtomicUsize,
        list_calls: AtomicUsize,
        probe_calls: AtomicUsize,
    }

    impl ScriptedFs {
        fn read(self, values: Vec<io::Result<Vec<u8>>>) -> Self {
            *self.reads.lock().unwrap() = values.into();
            self
        }

        fn list(self, values: Vec<io::Result<Vec<PathBuf>>>) -> Self {
            *self.listings.lock().unwrap() = values.into();
            self
        }

        fn probe(self, values: Vec<io::Result<()>>) -> Self {
            *self.probes.lock().unwrap() = values.into();
            self
        }
    }

    impl ReaderFs for ScriptedFs {
        async fn read_dir(&self, _path: &Path) -> io::Result<Vec<PathBuf>> {
            self.list_calls.fetch_add(1, Ordering::SeqCst);
            self.listings
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(Vec::new()))
        }

        async fn probe(&self, _path: &Path) -> io::Result<()> {
            self.probe_calls.fetch_add(1, Ordering::SeqCst);
            self.probes.lock().unwrap().pop_front().unwrap_or(Ok(()))
        }

        async fn read_file(&self, _path: &Path) -> io::Result<Vec<u8>> {
            self.read_calls.fetch_add(1, Ordering::SeqCst);
            self.reads
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(Vec::new()))
        }
    }

    struct HangingFs;

    impl ReaderFs for HangingFs {
        async fn read_dir(&self, _path: &Path) -> io::Result<Vec<PathBuf>> {
            std::future::pending().await
        }

        async fn probe(&self, _path: &Path) -> io::Result<()> {
            std::future::pending().await
        }

        async fn read_file(&self, _path: &Path) -> io::Result<Vec<u8>> {
            std::future::pending().await
        }
    }

    #[tokio::test]
    async fn retries_transient_io_then_succeeds() {
        let hall = ScriptedFs::default().read(vec![
            Err(io::Error::from(io::ErrorKind::PermissionDenied)),
            Err(io::Error::from(io::ErrorKind::Interrupted)),
            Ok(b"ok".to_vec()),
        ]);
        let value = retry(SHORT, || hall.read_file(Path::new("x")))
            .await
            .unwrap();
        assert_eq!(value, b"ok");
        assert_eq!(hall.read_calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn parse_errors_are_not_retried() {
        let hall = ScriptedFs::default()
            .list(vec![Ok(vec![PathBuf::from("/r/x.json")])])
            .read(vec![Ok(b"not json".to_vec())]);
        let error = read_reference_dir(
            &hall,
            &registry(),
            Path::new("/r"),
            SHORT,
            &mut HashSet::new(),
        )
        .await
        .unwrap_err();
        assert!(!error.is_timeout());
        assert_eq!(hall.read_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_hung_listing_probe_or_read_times_out_after_one_attempt() {
        let policy = SHORT;
        for what in ["list", "probe", "read"] {
            let error = match what {
                "list" => tokio::time::timeout(
                    Duration::from_secs(1),
                    retry(policy, || HangingFs.read_dir(Path::new("x"))),
                )
                .await
                .unwrap()
                .unwrap_err(),
                "probe" => tokio::time::timeout(
                    Duration::from_secs(1),
                    retry(policy, || HangingFs.probe(Path::new("x"))),
                )
                .await
                .unwrap()
                .unwrap_err(),
                _ => tokio::time::timeout(
                    Duration::from_secs(1),
                    retry(policy, || HangingFs.read_file(Path::new("x"))),
                )
                .await
                .unwrap()
                .unwrap_err(),
            };
            assert!(error.is_timeout(), "{what}");
        }
    }

    #[tokio::test]
    async fn missing_listing_is_empty_and_missing_probe_is_not_live() {
        let hall = ScriptedFs::default()
            .list(vec![Err(io::Error::from(io::ErrorKind::NotFound))])
            .probe(vec![Err(io::Error::from(io::ErrorKind::NotFound))]);
        assert!(
            registration_refs_dirs(&hall, Path::new("/store"), SHORT)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(hall.list_calls.load(Ordering::SeqCst), 1);

        let hall = ScriptedFs::default()
            .probe(vec![Err(io::Error::from(io::ErrorKind::NotFound))]);
        assert!(
            !refs_dir_is_live(&hall, Path::new("/ws/refs"), SHORT)
                .await
                .unwrap()
        );
        assert_eq!(hall.probe_calls.load(Ordering::SeqCst), 1);

        let hall = ScriptedFs::default()
            .probe(vec![Err(io::Error::from(io::ErrorKind::Other)), Ok(())]);
        assert!(
            refs_dir_is_live(&hall, Path::new("/ws/refs"), SHORT)
                .await
                .unwrap()
        );
        assert_eq!(hall.probe_calls.load(Ordering::SeqCst), 2);

        let error = refs_dir_is_live(&HangingFs, Path::new("/ws/refs"), SHORT)
            .await
            .unwrap_err();
        assert!(error.is_timeout());
    }

    #[tokio::test]
    async fn a_listed_file_that_disappears_retries_then_fails_closed() {
        let hall = ScriptedFs::default().read(vec![
            Err(io::Error::from(io::ErrorKind::NotFound)),
            Err(io::Error::from(io::ErrorKind::NotFound)),
            Err(io::Error::from(io::ErrorKind::NotFound)),
        ]);
        let error = retry(SHORT, || hall.read_file(Path::new("x")))
            .await
            .unwrap_err();
        assert!(!error.is_timeout());
        assert_eq!(hall.read_calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn a_listed_file_that_appears_after_a_retry_is_read() {
        let hall = ScriptedFs::default().read(vec![
            Err(io::Error::from(io::ErrorKind::NotFound)),
            Ok(b"ok".to_vec()),
        ]);
        let value = retry(SHORT, || hall.read_file(Path::new("x")))
            .await
            .unwrap();
        assert_eq!(value, b"ok");
        assert_eq!(hall.read_calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn a_probe_live_then_listing_not_found_reads_as_empty() {
        // The directory vanished between the liveness probe and the listing.
        let hall = ScriptedFs::default()
            .list(vec![Err(io::Error::from(io::ErrorKind::NotFound))]);
        read_reference_dir(
            &hall,
            &registry(),
            Path::new("/ws/refs"),
            SHORT,
            &mut HashSet::new(),
        )
        .await
        .unwrap();
        assert_eq!(hall.list_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_timeout_stops_further_reads() {
        let mut state = RegistryState::default();
        let key = ("git".into(), "remote".into(), "pin".into());
        let first = recheck(
            &HangingFs,
            &registry(),
            Path::new("/store"),
            Path::new("/ws/refs"),
            SHORT,
            &mut state,
            &key,
        )
        .await;
        assert!(
            matches!(&first, Recheck::Failed(message) if message.contains("timed out")),
            "{first:?}"
        );
        assert!(state.stopped().is_some());

        let second = ScriptedFs::default();
        assert_eq!(
            recheck(
                &second,
                &registry(),
                Path::new("/store"),
                Path::new("/ws/refs"),
                SHORT,
                &mut state,
                &key,
            )
            .await,
            Recheck::Failed(state.stopped().unwrap().to_string())
        );
        assert_eq!(second.list_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn unknown_kinds_are_ignored_and_unsafe_pins_fail_closed() {
        let mut data = RefSetData::default();
        let RefSetData::V1_0_0(inner) = &mut data;
        inner.insert("unknown", "loc", "sel", "../unsafe");
        insert_reference_keys(
            &registry(),
            Path::new("refs/x.json"),
            &data,
            &mut HashSet::new(),
        )
        .unwrap();

        let mut data = RefSetData::default();
        let RefSetData::V1_0_0(inner) = &mut data;
        inner.insert("git", "loc", "sel", "../unsafe");
        let error = insert_reference_keys(
            &registry(),
            Path::new("refs/x.json"),
            &data,
            &mut HashSet::new(),
        )
        .unwrap_err();
        assert!(error.message().contains("unsafe reference pin"));
    }

    #[tokio::test]
    async fn a_stopped_registry_skips_further_reads() {
        let hall = ScriptedFs::default();
        let mut state = RegistryState::default();
        state.stop("timed out");
        let key = ("git".into(), "remote".into(), "pin".into());

        assert_eq!(
            recheck(
                &hall,
                &registry(),
                Path::new("/store"),
                Path::new("/ws/refs"),
                SHORT,
                &mut state,
                &key,
            )
            .await,
            Recheck::Failed("timed out".into())
        );
        assert_eq!(hall.list_calls.load(Ordering::SeqCst), 0);
    }
}
