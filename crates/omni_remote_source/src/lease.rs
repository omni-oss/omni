//! Low-level checkout retention locks. These coordinate cooperating processes;
//! they do not protect against external deletion or grant access to a source.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use omni_utils::lock::LockGuard;

use crate::{driver::is_safe_segment, error::Error};

/// A process-held shared lock for one immutable checkout.
///
/// Share with `Arc<SourceLease>` when several runtime objects read the checkout.
/// The last owner dropping it (or process exit) releases the OS lock. Empty
/// lease files are deliberately never removed: unlinking can let two openers
/// lock different files at the same path.
///
/// This primitive alone does not make existing materialization or pruning
/// lease-aware. Callers must implement the acquisition/GC protocol before
/// relying on it for retention.
#[must_use = "dropping the lease releases checkout retention"]
pub struct SourceLease {
    _guard: LockGuard,
    /// The lease file this handle locks, so a [`LeaseSet`] can drop duplicates
    /// by checkout, including the store root.
    path: PathBuf,
}

impl std::fmt::Debug for SourceLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceLease").finish_non_exhaustive()
    }
}

impl SourceLease {
    /// Acquire a shared checkout lease.
    ///
    /// This is a low-level protocol operation, not a safe way to protect an
    /// already-returned checkout: acquire before checking completeness on a
    /// fast path, or while holding the remote lock when publishing. Never
    /// acquire the remote lock while holding the workspace advisory lock.
    /// `slug` must be the canonical on-disk slug, not a driver's raw slug.
    /// Invalid path segments and filesystem/locking failures return errors.
    pub async fn acquire(
        store_root: &Path,
        kind: &str,
        slug: &str,
        pin: &str,
    ) -> Result<Self, Error> {
        let path = Self::path(store_root, kind, slug, pin)?;
        let guard = LockGuard::acquire_shared(&path).await?;
        Ok(Self {
            _guard: guard,
            path,
        })
    }

    fn file(&self) -> &Path {
        &self.path
    }

    /// Try an exclusive lease for a collector. `None` means a reader holds it.
    ///
    /// Hold the remote lock before calling this and keep the returned guard
    /// until deletion finishes. Never unlink the lease file or its parents.
    /// `slug` must be the canonical on-disk slug. Invalid path segments and
    /// filesystem/locking failures return errors, not contention.
    pub async fn try_acquire_exclusive(
        store_root: &Path,
        kind: &str,
        slug: &str,
        pin: &str,
    ) -> Result<Option<LockGuard>, Error> {
        let path = Self::path(store_root, kind, slug, pin)?;
        Ok(LockGuard::try_acquire_exclusive(path).await?)
    }

    /// Whether a lease is currently held, without creating anything. For a dry
    /// run: an absent lease file means no holder, and a present one is probed
    /// with a non-blocking exclusive lock that is released immediately. The
    /// answer is a snapshot; a real prune still takes the lease itself.
    pub async fn is_held(
        store_root: &Path,
        kind: &str,
        slug: &str,
        pin: &str,
    ) -> Result<bool, Error> {
        let path = Self::path(store_root, kind, slug, pin)?;
        if !tokio::fs::try_exists(&path).await? {
            return Ok(false);
        }
        Ok(LockGuard::try_acquire_exclusive(path).await?.is_none())
    }

    fn path(
        store_root: &Path,
        kind: &str,
        slug: &str,
        pin: &str,
    ) -> Result<PathBuf, Error> {
        for (name, value) in [("kind", kind), ("slug", slug), ("pin", pin)] {
            if !is_safe_segment(value) {
                return Err(Error::custom(format!(
                    "unsafe source lease {name}: {value:?}"
                )));
            }
        }
        Ok(store_root
            .join("leases")
            .join(kind)
            .join(slug)
            .join(format!("{pin}.lease")))
    }
}

/// The leases one operation holds, released together when the last handle drops.
///
/// An operation that reads store checkouts (a generator run, a projection sync)
/// collects a lease per materialized source here and keeps the set alive until
/// it has finished reading. Clones share one set, so a spawned task can add to
/// it. Bind the set to a named variable (`let _leases = ...`, never `_`): an
/// unbound set drops at once and protects nothing.
///
/// One checkout can be materialized more than once in one operation (a source
/// shared by two subsystems, or a pack expanded twice). The set keeps only the
/// first lease for each `(kind, slug, pin)`: a duplicate is dropped immediately,
/// closing its descriptor. `flock`-style locks are per open file description, so
/// dropping a duplicate never releases the kept handle's lock.
#[derive(Debug, Clone, Default)]
#[must_use = "dropping the set releases every lease it holds"]
pub struct LeaseSet(Arc<Mutex<LeaseSetInner>>);

#[derive(Debug, Default)]
struct LeaseSetInner {
    leases: Vec<Arc<SourceLease>>,
    keys: HashSet<PathBuf>,
}

impl LeaseSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Keep `lease` held for as long as any handle to this set lives. A lease
    /// for a checkout the set already holds is dropped at once.
    pub fn hold(&self, lease: Arc<SourceLease>) {
        // The set is only appended to, so a poisoned lock leaves it consistent.
        let mut inner = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if inner.keys.insert(lease.file().to_path_buf()) {
            inner.leases.push(lease);
        }
    }

    /// Move every lease `other` holds into this set. `other` keeps nothing, so
    /// protection follows the operation that outlives the helper which took it.
    /// Duplicates are dropped and never hold both mutexes at once, so two sets
    /// absorbing each other cannot deadlock.
    pub fn absorb(&self, other: &LeaseSet) {
        if Arc::ptr_eq(&self.0, &other.0) {
            return;
        }
        let taken = std::mem::take(
            &mut *other.0.lock().unwrap_or_else(|e| e.into_inner()),
        );
        let mut inner = self.0.lock().unwrap_or_else(|e| e.into_inner());
        for lease in taken.leases {
            if inner.keys.insert(lease.file().to_path_buf()) {
                inner.leases.push(lease);
            }
        }
    }

    /// The number of distinct checkouts the set keeps alive.
    pub fn len(&self) -> usize {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .leases
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
