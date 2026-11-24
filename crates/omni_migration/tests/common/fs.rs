use std::collections::{HashMap, VecDeque};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use omni_migration::{BackendSupport, Error};

pub const FAILPOINT_ENV: &str = "OMNI_MIG_FAILPOINTS";

#[derive(Clone)]
pub enum Action {
    /// Fail the operation with an I/O error.
    Err,
    /// Simulate process death at this point.
    Exit,
    /// Signal `ready`, then block until `release` exists.
    Wait { ready: PathBuf, release: PathBuf },
    /// Run a callback, then continue. Simulates a peer acting at this point.
    Hook(Arc<dyn Fn() + Send + Sync>),
}

#[derive(Clone)]
struct Spec {
    action: Action,
    skip: u32,
}

#[derive(Default)]
struct Inner {
    points: Mutex<HashMap<String, Spec>>,
    live: Mutex<Option<BackendSupport>>,
    probe_script: Mutex<VecDeque<BackendSupport>>,
    mutations: AtomicU64,
    temp_counter: AtomicU64,
}

/// Fault-injecting handle shared between the fixtures and the tests.
#[derive(Clone, Default)]
pub struct Faults(Arc<Inner>);

impl Faults {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse `name=err|exit|wait:<ready>:<release>[@skip],...`.
    pub fn from_env() -> Self {
        let faults = Self::new();
        if let Ok(v) = std::env::var(FAILPOINT_ENV) {
            for item in v.split(',').filter(|s| !s.is_empty()) {
                let (name, rest) = item.split_once('=').expect("name=action");
                let (action, skip) = match rest.rsplit_once('@') {
                    Some((a, s)) => (a, s.parse().expect("skip")),
                    None => (rest, 0),
                };
                let action = match action.split(':').collect::<Vec<_>>()[..] {
                    ["err"] => Action::Err,
                    ["exit"] => Action::Exit,
                    ["wait", ready, release] => Action::Wait {
                        ready: ready.into(),
                        release: release.into(),
                    },
                    _ => panic!("bad failpoint action {action}"),
                };
                faults.set(name, action, skip);
            }
        }
        faults
    }

    pub fn set(&self, name: &str, action: Action, skip: u32) {
        self.0
            .points
            .lock()
            .unwrap()
            .insert(name.to_string(), Spec { action, skip });
    }

    pub fn clear(&self) {
        self.0.points.lock().unwrap().clear();
    }

    pub fn set_support(&self, support: BackendSupport) {
        *self.0.live.lock().unwrap() = Some(support);
    }

    /// Each `probe()` pops the next scripted value; once empty it reports the
    /// live value.
    pub fn script_probes(&self, values: &[BackendSupport]) {
        *self.0.probe_script.lock().unwrap() = values.iter().copied().collect();
    }

    pub fn probe(&self) -> BackendSupport {
        if let Some(v) = self.0.probe_script.lock().unwrap().pop_front() {
            return v;
        }
        self.0.live.lock().unwrap().unwrap_or(BackendSupport::FULL)
    }

    /// Count of format/data writes (excludes guard acquisition and reads).
    pub fn mutations(&self) -> u64 {
        self.0.mutations.load(Ordering::SeqCst)
    }

    pub(crate) fn mutate(&self) {
        self.0.mutations.fetch_add(1, Ordering::SeqCst);
    }

    pub(crate) fn temp_id(&self) -> u64 {
        self.0.temp_counter.fetch_add(1, Ordering::SeqCst)
    }

    /// Hit a named failpoint.
    pub fn hit(&self, name: &str) -> Result<(), Error> {
        let action = {
            let mut points = self.0.points.lock().unwrap();
            match points.get_mut(name) {
                None => return Ok(()),
                Some(spec) if spec.skip > 0 => {
                    spec.skip -= 1;
                    return Ok(());
                }
                Some(spec) => spec.action.clone(),
            }
        };
        match action {
            Action::Err => {
                Err(Error::Io(format!("injected failure at {name}")))
            }
            Action::Exit => std::process::exit(86),
            Action::Hook(f) => {
                f();
                Ok(())
            }
            Action::Wait { ready, release } => {
                std::fs::write(&ready, b"ready")?;
                while !release.exists() {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Ok(())
            }
        }
    }
}

pub enum PublishError {
    /// Nothing visible changed.
    BeforeRename(Error),
    /// The rename may or may not have happened and may not be durable.
    Uncertain(Error),
}

impl PublishError {
    pub fn into_error(self) -> Error {
        match self {
            Self::BeforeRename(e) | Self::Uncertain(e) => e,
        }
    }
}

pub struct Staged {
    path: PathBuf,
}

/// Exclusive same-directory staging, file sync, atomic rename and directory
/// sync, with failpoints `<tag>.stage`, `<tag>.file_sync`, `<tag>.rename` and
/// `<tag>.parent_sync`.
#[derive(Clone)]
pub struct DurableFs {
    pub faults: Faults,
}

pub struct Guard {
    _file: File,
}

impl DurableFs {
    pub fn new(faults: Faults) -> Self {
        Self { faults }
    }

    pub fn probe(&self) -> BackendSupport {
        self.faults.probe()
    }

    pub fn lock_exclusive(&self, path: &Path) -> Result<Guard, Error> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)?;
        file.lock()?;
        Ok(Guard { _file: file })
    }

    pub fn try_lock_exclusive(&self, path: &Path) -> Result<Guard, Error> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)?;
        match file.try_lock() {
            Ok(()) => Ok(Guard { _file: file }),
            Err(std::fs::TryLockError::WouldBlock) => Err(Error::GuardBusy),
            Err(std::fs::TryLockError::Error(e)) => Err(e.into()),
        }
    }

    pub fn sync_dir(&self, dir: &Path) -> Result<(), Error> {
        File::open(dir)?.sync_all()?;
        Ok(())
    }

    pub fn stage(
        &self,
        dest: &Path,
        bytes: &[u8],
        tag: &str,
    ) -> Result<Staged, Error> {
        self.faults.hit(&format!("{tag}.stage"))?;
        self.faults.mutate();
        let dir = dest.parent().expect("dest has parent");
        let path = dir.join(format!(
            ".stage-{}-{}",
            std::process::id(),
            self.faults.temp_id()
        ));
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        f.write_all(bytes)?;
        self.faults.hit(&format!("{tag}.file_sync"))?;
        f.sync_all()?;
        Ok(Staged { path })
    }

    pub fn replace(
        &self,
        staged: Staged,
        dest: &Path,
        tag: &str,
    ) -> Result<(), PublishError> {
        self.faults
            .hit(&format!("{tag}.rename"))
            .map_err(PublishError::BeforeRename)?;
        self.faults.mutate();
        std::fs::rename(&staged.path, dest)
            .map_err(|e| PublishError::BeforeRename(e.into()))?;
        let sync = self
            .faults
            .hit(&format!("{tag}.parent_sync"))
            .and_then(|_| self.sync_dir(dest.parent().unwrap()));
        sync.map_err(PublishError::Uncertain)
    }

    pub fn write_atomic(
        &self,
        dest: &Path,
        bytes: &[u8],
        tag: &str,
    ) -> Result<(), PublishError> {
        let staged = self
            .stage(dest, bytes, tag)
            .map_err(PublishError::BeforeRename)?;
        self.replace(staged, dest, tag)
    }

    /// Unlink then sync the parent. `Ok(false)` means removal is visible but
    /// its directory sync failed.
    pub fn remove_and_sync(
        &self,
        path: &Path,
        tag: &str,
    ) -> Result<bool, Error> {
        self.faults.hit(&format!("{tag}.unlink"))?;
        self.faults.mutate();
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        let sync = self
            .faults
            .hit(&format!("{tag}.parent_sync"))
            .and_then(|_| self.sync_dir(path.parent().unwrap()));
        Ok(sync.is_ok())
    }
}

pub fn fingerprint(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}
