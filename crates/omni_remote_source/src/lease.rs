//! Low-level checkout retention locks. These coordinate cooperating processes;
//! they do not protect against external deletion or grant access to a source.

use std::path::{Path, PathBuf};

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
        Ok(Self {
            _guard: LockGuard::acquire_shared(path).await?,
        })
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
