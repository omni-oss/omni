//! [`PolicyEnforcingSys`]: a transparent decorator over a system handle that
//! authorizes every filesystem operation against a capability policy before
//! delegating to the wrapped `TSys`.
//!
//! ## How it works
//!
//! For each `system_traits` trait that `TSys` implements, `PolicyEnforcingSys`
//! provides a matching implementation that:
//!
//! 1. builds the [`Request`] describing the operation (an `fs.read` or
//!    `fs.write` against a concrete path),
//! 2. asks its [`CapabilityAuthorizer`] to decide, and
//! 3. on `Deny`, returns an [`io::ErrorKind::PermissionDenied`] error carrying
//!    the "show why" reason; on `Allow`, forwards to the inner handle.
//!
//! The impls are **conditional**: `PolicyEnforcingSys<TSys, A>` implements a
//! trait only when `TSys` does, so wrapping never removes capabilities — it
//! only gates the ones present.
//!
//! ## What is and isn't gated
//!
//! * **Filesystem** reads/writes are gated (the primary attack surface for a
//!   hijacked script). Directory globbing ([`BaseFsGlobAsync`]) is treated as a
//!   recursive read: the root must be readable and each discovered path is
//!   filtered to those the policy permits, so a `**` glob cannot disclose files
//!   in denied subtrees.
//! * **`env`** is not gated per-operation here: [`EnvVars::env_vars`] yields an
//!   opaque `std::env::Vars`, which cannot be filtered without materializing a
//!   map. The enforcing wrapper therefore does **not** implement [`EnvVars`] at
//!   all; it exposes only the filtered [`EnvSnapshot::env_snapshot`], so a
//!   consumer (the RPC env service) cannot accidentally read the raw
//!   environment past the policy. Current-directory reads and changes are
//!   passed through.
//!
//! This decorator is the concrete [`Tier::InProcessBroker`] mechanism: because
//! it authorizes *every* mediated operation against the full policy, it
//! enforces arbitrary patterns (globs, denials) that coarse pre-spawn backends
//! cannot — provided the runtime cannot bypass it with direct syscalls.
//!
//! [`Tier::InProcessBroker`]: omni_capabilities
//! [`Request`]: omni_capabilities::Request

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use omni_capabilities::{Decision, DenyReason, Request};
use system_traits::{
    BaseEnvSetCurrentDirAsync, BaseFsAppendAsync, BaseFsCanonicalizeAsync,
    BaseFsCopyAsync, BaseFsCreateDirAsync, BaseFsGlobAsync,
    BaseFsHardLinkAsync, BaseFsMetadataAsync, BaseFsReadAsync,
    BaseFsReadDirAsync, BaseFsRemoveDirAllAsync, BaseFsRemoveDirAsync,
    BaseFsRemoveFileAsync, BaseFsRenameAsync, BaseFsWriteAsync,
    CreateDirOptions, EnvCurrentDirAsync, EnvSnapshot, EnvVars,
};

use crate::{CapabilityAuthorizer, EnvAccess};

/// A system handle that enforces a capability policy on filesystem operations.
///
/// Construct it with [`PolicyEnforcingSys::new`], typically wrapping a
/// `RealSys` and an [`EvaluatingAuthorizer`](crate::EvaluatingAuthorizer), then
/// hand it to whatever consumes a `system_traits` handle (e.g. the bridge RPC
/// filesystem services) in place of the raw sys.
#[derive(Clone)]
pub struct PolicyEnforcingSys<TSys, A> {
    inner: TSys,
    authorizer: A,
    env: EnvAccess,
}

impl<TSys, A> PolicyEnforcingSys<TSys, A> {
    pub fn new(inner: TSys, authorizer: A) -> Self {
        Self {
            inner,
            authorizer,
            env: EnvAccess::default(),
        }
    }

    /// Set how environment access is confined (see [`EnvAccess`]). Defaults to
    /// [`EnvAccess::Filter`].
    pub fn with_env_access(mut self, env: EnvAccess) -> Self {
        self.env = env;
        self
    }

    /// The configured environment-access mode.
    pub fn env_access(&self) -> EnvAccess {
        self.env
    }

    /// Borrow the wrapped handle.
    pub fn inner(&self) -> &TSys {
        &self.inner
    }

    /// Recover the wrapped handle, discarding the policy layer.
    pub fn into_inner(self) -> TSys {
        self.inner
    }
}

impl<TSys, A: CapabilityAuthorizer> PolicyEnforcingSys<TSys, A> {
    fn guard_read(&self, path: &Path) -> io::Result<()> {
        self.guard(false, path)
    }

    fn guard_write(&self, path: &Path) -> io::Result<()> {
        self.guard(true, path)
    }

    /// Authorize a filesystem operation on `path`.
    ///
    /// Authorization runs against the operation's **symlink-resolved real
    /// path** (see [`resolve_real_path`]), not merely the lexical path the
    /// caller passed. This closes the symlink-escape hole: a symlink created
    /// inside an allowed subtree that points outside it (e.g.
    /// `@workspace/evil -> /etc/passwd`) matches the allow-list *lexically*, but
    /// its resolved target does not, so it is denied. For a not-yet-existing
    /// write target the longest existing ancestor is resolved and the remainder
    /// re-appended, so a symlinked *parent directory* is caught too.
    ///
    /// When the path cannot be resolved at all (it and every ancestor are
    /// absent, or the platform cannot canonicalize) the original lexical path is
    /// authorized instead — the backstop is best-effort and never *loosens* a
    /// decision. A purely synthetic path therefore resolves to itself, which is
    /// what keeps in-memory / unit-test paths behaving exactly as before.
    ///
    /// This does **not**, on its own, close the residual TOCTOU race (a
    /// component swapped to a symlink between this check and the eventual
    /// syscall). The un-bypassable floor for that is the OS sandbox (Landlock on
    /// Linux), which re-resolves the real path in-kernel at open time; this
    /// broker check is the (cross-platform) defense-in-depth layer above it.
    ///
    /// Re-authorizing the resolved path relies on the authorizer's roots being
    /// canonical, which the enforcement layer arranges (see the generator script
    /// runner, which canonicalizes root bases). Otherwise a root that itself
    /// lives under a symlink could be misread as an escape.
    fn guard(&self, write: bool, path: &Path) -> io::Result<()> {
        let resolved = resolve_real_path(path);
        let target = resolved.as_deref().unwrap_or(path);
        match self.authorizer.authorize(&Request::Fs {
            write,
            path: target,
        }) {
            Decision::Allow => Ok(()),
            Decision::Deny(reason) => {
                Err(permission_denied(&reason, path, target))
            }
        }
    }

    fn read_allowed(&self, path: &Path) -> bool {
        let resolved = resolve_real_path(path);
        let target = resolved.as_deref().unwrap_or(path);
        matches!(
            self.authorizer.authorize(&Request::Fs {
                write: false,
                path: target
            }),
            Decision::Allow
        )
    }
}

impl<TSys, A> PolicyEnforcingSys<TSys, A>
where
    TSys: EnvVars,
    A: CapabilityAuthorizer,
{
    /// Whether a single environment variable `name` is permitted under the
    /// current [`EnvAccess`] mode.
    pub fn env_allows(&self, name: &str) -> bool {
        match self.env {
            EnvAccess::Passthrough => true,
            EnvAccess::Filter => matches!(
                self.authorizer.authorize(&Request::Env { name }),
                Decision::Allow
            ),
        }
    }
}

/// Render a policy denial as an `io::Error` a caller (and, ultimately, the
/// script) can surface. The message is intentionally explicit about *why*, and
/// — when the requested path resolved (through a symlink) to a different real
/// path than the one asked for — names both, so a symlink-escape denial is not
/// mistaken for a plain out-of-scope one.
fn permission_denied(
    reason: &DenyReason,
    requested: &Path,
    resolved: &Path,
) -> io::Error {
    let message = if resolved == requested {
        format!(
            "capability policy denied `{}` access to `{}` ({:?})",
            reason.domain,
            requested.display(),
            reason.cause
        )
    } else {
        format!(
            "capability policy denied `{}` access to `{}`: it resolves to `{}`, \
             which is outside the permitted paths ({:?})",
            reason.domain,
            requested.display(),
            resolved.display(),
            reason.cause
        )
    };
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

/// Resolve `path` to its real on-disk location, following symlinks in every
/// component, so an operation can be authorized against the target it will
/// actually reach rather than the lexical path requested.
///
/// * A fully existing path is canonicalized directly.
/// * A not-yet-existing target (e.g. a write that creates a new file) has its
///   longest existing ancestor canonicalized, with the non-existent remainder
///   re-appended — so a symlinked parent directory is still resolved.
/// * If neither the path nor any ancestor can be resolved (a purely synthetic
///   path, or a platform without canonicalization) `None` is returned and the
///   caller falls back to the lexical path. A synthetic *absolute* path
///   therefore resolves to itself (its absent components are appended onto the
///   real filesystem root), which keeps in-memory / unit-test paths behaving
///   exactly as before.
///
/// Uses a single blocking `canonicalize` syscall per unresolved ancestor; this
/// is negligible relative to the mediated fs operation that follows.
fn resolve_real_path(path: &Path) -> Option<PathBuf> {
    if let Ok(real) = std::fs::canonicalize(path) {
        return Some(real);
    }
    // Walk up to the longest existing ancestor, collecting the absent tail.
    let mut tail: Vec<OsString> = Vec::new();
    let mut current = path;
    while let Some(parent) = current.parent() {
        // A path with no final component (e.g. `/`) cannot be split further.
        let name = current.file_name()?;
        tail.push(name.to_owned());
        if let Ok(real_parent) = std::fs::canonicalize(parent) {
            let mut resolved = real_parent;
            for name in tail.iter().rev() {
                resolved.push(name.as_os_str());
            }
            return Some(resolved);
        }
        current = parent;
    }
    None
}

// ── filesystem: reads ────────────────────────────────────────────────────────

#[async_trait]
impl<TSys, A> BaseFsReadAsync for PolicyEnforcingSys<TSys, A>
where
    TSys: BaseFsReadAsync + Send + Sync,
    A: CapabilityAuthorizer,
{
    async fn base_fs_read_async(
        &self,
        path: &Path,
    ) -> io::Result<Cow<'static, [u8]>> {
        self.guard_read(path)?;
        self.inner.base_fs_read_async(path).await
    }
}

#[async_trait]
impl<TSys, A> BaseFsMetadataAsync for PolicyEnforcingSys<TSys, A>
where
    TSys: BaseFsMetadataAsync + Send + Sync,
    A: CapabilityAuthorizer,
{
    type Metadata = TSys::Metadata;

    async fn base_fs_metadata_async(
        &self,
        path: &Path,
    ) -> io::Result<Self::Metadata> {
        self.guard_read(path)?;
        self.inner.base_fs_metadata_async(path).await
    }

    async fn base_fs_symlink_metadata_async(
        &self,
        path: &Path,
    ) -> io::Result<Self::Metadata> {
        self.guard_read(path)?;
        self.inner.base_fs_symlink_metadata_async(path).await
    }
    // `base_fs_exists_async` uses the default, which calls the gated
    // `base_fs_symlink_metadata_async` above — so existence checks are gated too.
}

#[async_trait]
impl<TSys, A> BaseFsReadDirAsync for PolicyEnforcingSys<TSys, A>
where
    TSys: BaseFsReadDirAsync + Send + Sync,
    A: CapabilityAuthorizer,
{
    async fn base_fs_read_dir_async(
        &self,
        path: &Path,
    ) -> io::Result<Vec<PathBuf>> {
        self.guard_read(path)?;
        self.inner.base_fs_read_dir_async(path).await
    }
}

#[async_trait]
impl<TSys, A> BaseFsCanonicalizeAsync for PolicyEnforcingSys<TSys, A>
where
    TSys: BaseFsCanonicalizeAsync + Send + Sync,
    A: CapabilityAuthorizer,
{
    async fn base_fs_canonicalize_async(
        &self,
        path: &Path,
    ) -> io::Result<PathBuf> {
        self.guard_read(path)?;
        self.inner.base_fs_canonicalize_async(path).await
    }
}

#[async_trait]
impl<TSys, A> BaseFsGlobAsync for PolicyEnforcingSys<TSys, A>
where
    TSys: BaseFsGlobAsync + Send + Sync,
    A: CapabilityAuthorizer,
{
    async fn base_fs_glob_async(
        &self,
        root_dir: &Path,
        patterns: &[&str],
    ) -> io::Result<Vec<PathBuf>> {
        // Globbing is a recursive read/discovery: you must be allowed to read
        // the root being searched...
        self.guard_read(root_dir)?;
        let matches = self.inner.base_fs_glob_async(root_dir, patterns).await?;
        // ...and each discovered path is filtered to those the policy permits
        // reading, so a `**` pattern cannot disclose files in denied subtrees.
        Ok(matches
            .into_iter()
            .filter(|path| self.read_allowed(path.as_path()))
            .collect())
    }
}

// ── filesystem: writes ───────────────────────────────────────────────────────

#[async_trait]
impl<TSys, A> BaseFsWriteAsync for PolicyEnforcingSys<TSys, A>
where
    TSys: BaseFsWriteAsync + Send + Sync,
    A: CapabilityAuthorizer,
{
    async fn base_fs_write_async(
        &self,
        path: &Path,
        data: &[u8],
    ) -> io::Result<()> {
        self.guard_write(path)?;
        self.inner.base_fs_write_async(path, data).await
    }
}

#[async_trait]
impl<TSys, A> BaseFsAppendAsync for PolicyEnforcingSys<TSys, A>
where
    TSys: BaseFsAppendAsync + Send + Sync,
    A: CapabilityAuthorizer,
{
    async fn base_fs_append_async(
        &self,
        path: &Path,
        data: &[u8],
    ) -> io::Result<()> {
        self.guard_write(path)?;
        self.inner.base_fs_append_async(path, data).await
    }
}

#[async_trait]
impl<TSys, A> BaseFsCreateDirAsync for PolicyEnforcingSys<TSys, A>
where
    TSys: BaseFsCreateDirAsync + Send + Sync,
    A: CapabilityAuthorizer,
{
    async fn base_fs_create_dir_async(
        &self,
        path: &Path,
        options: &CreateDirOptions,
    ) -> io::Result<()> {
        self.guard_write(path)?;
        self.inner.base_fs_create_dir_async(path, options).await
    }
}

#[async_trait]
impl<TSys, A> BaseFsRemoveFileAsync for PolicyEnforcingSys<TSys, A>
where
    TSys: BaseFsRemoveFileAsync + Send + Sync,
    A: CapabilityAuthorizer,
{
    async fn base_fs_remove_file_async(&self, path: &Path) -> io::Result<()> {
        self.guard_write(path)?;
        self.inner.base_fs_remove_file_async(path).await
    }
}

#[async_trait]
impl<TSys, A> BaseFsRemoveDirAsync for PolicyEnforcingSys<TSys, A>
where
    TSys: BaseFsRemoveDirAsync + Send + Sync,
    A: CapabilityAuthorizer,
{
    async fn base_fs_remove_dir_async(&self, path: &Path) -> io::Result<()> {
        self.guard_write(path)?;
        self.inner.base_fs_remove_dir_async(path).await
    }
}

#[async_trait]
impl<TSys, A> BaseFsRemoveDirAllAsync for PolicyEnforcingSys<TSys, A>
where
    TSys: BaseFsRemoveDirAllAsync + Send + Sync,
    A: CapabilityAuthorizer,
{
    async fn base_fs_remove_dir_all_async(
        &self,
        path: &Path,
    ) -> io::Result<()> {
        self.guard_write(path)?;
        self.inner.base_fs_remove_dir_all_async(path).await
    }
}

#[async_trait]
impl<TSys, A> BaseFsRenameAsync for PolicyEnforcingSys<TSys, A>
where
    TSys: BaseFsRenameAsync + Send + Sync,
    A: CapabilityAuthorizer,
{
    async fn base_fs_rename_async(
        &self,
        from: &Path,
        to: &Path,
    ) -> io::Result<()> {
        // A rename removes `from` and creates `to`: both are writes.
        self.guard_write(from)?;
        self.guard_write(to)?;
        self.inner.base_fs_rename_async(from, to).await
    }
}

#[async_trait]
impl<TSys, A> BaseFsCopyAsync for PolicyEnforcingSys<TSys, A>
where
    TSys: BaseFsCopyAsync + Send + Sync,
    A: CapabilityAuthorizer,
{
    async fn base_fs_copy_async(
        &self,
        from: &Path,
        to: &Path,
    ) -> io::Result<u64> {
        // Copy reads the source and writes the destination.
        self.guard_read(from)?;
        self.guard_write(to)?;
        self.inner.base_fs_copy_async(from, to).await
    }
}

#[async_trait]
impl<TSys, A> BaseFsHardLinkAsync for PolicyEnforcingSys<TSys, A>
where
    TSys: BaseFsHardLinkAsync + Send + Sync,
    A: CapabilityAuthorizer,
{
    async fn base_fs_hard_link_async(
        &self,
        src: &Path,
        dst: &Path,
    ) -> io::Result<()> {
        // Reads the source inode, creates a new link at the destination.
        self.guard_read(src)?;
        self.guard_write(dst)?;
        self.inner.base_fs_hard_link_async(src, dst).await
    }
}

// ── pass-through: not capability-gated here ──────────────────────────────────

#[async_trait]
impl<TSys, A> EnvCurrentDirAsync for PolicyEnforcingSys<TSys, A>
where
    TSys: EnvCurrentDirAsync + Send + Sync,
    A: Send + Sync,
{
    async fn env_current_dir_async(&self) -> io::Result<PathBuf> {
        self.inner.env_current_dir_async().await
    }
}

#[async_trait]
impl<TSys, A> BaseEnvSetCurrentDirAsync for PolicyEnforcingSys<TSys, A>
where
    TSys: BaseEnvSetCurrentDirAsync + Send + Sync,
    A: Send + Sync,
{
    async fn base_env_set_current_dir_async(
        &self,
        path: &Path,
    ) -> io::Result<()> {
        self.inner.base_env_set_current_dir_async(path).await
    }
}

/// Materialize the environment as an owned map, applying the configured
/// [`EnvAccess`] policy.
///
/// This is the **only** way to read the environment through the enforcing
/// wrapper: it deliberately does not implement [`EnvVars`], whose
/// `std::env::Vars` return type cannot be filtered in place and would otherwise
/// leak the raw process environment. Under [`EnvAccess::Filter`] every variable
/// whose name the policy denies is dropped; under [`EnvAccess::Passthrough`]
/// the full environment is returned.
impl<TSys, A> EnvSnapshot for PolicyEnforcingSys<TSys, A>
where
    TSys: EnvVars,
    A: CapabilityAuthorizer,
{
    fn env_snapshot(&self) -> BTreeMap<String, String> {
        self.inner
            .env_vars()
            .filter(|(name, _)| self.env_allows(name))
            .collect()
    }
}
