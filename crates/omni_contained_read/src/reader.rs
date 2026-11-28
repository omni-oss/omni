use std::{
    borrow::Cow,
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

use async_trait::async_trait;
use omni_capability_sys::CapabilityAuthorizer;
use omni_input_view::{
    AuthDecision, Cancellation, InputAuthorizer, InputError, InputLimits,
    InputView, InputViewOptions, Read, SymlinkPolicy,
};
use omni_path_utils::{WorkspacePath, WorkspacePathError};
use system_traits::{BaseFsReadAsync, EnvCurrentDirAsync};

use crate::{
    CapabilityInputAuthorizer, error::to_io_error, resolve::resolve_real_path,
};

/// The contained disk read handed to a [`ReadThrough`] for a path that is not
/// answered from the sys's own state.
#[async_trait]
pub trait RealRead: Send + Sync {
    async fn read(&self, path: &Path) -> io::Result<Cow<'static, [u8]>>;
}

/// The seam for a sys that has state of its own (the generator overlay of
/// uncommitted writes and deletions) to answer before the disk is touched.
#[async_trait]
pub trait ReadThrough: Send + Sync {
    /// Make `path` absolute the way this sys does (its own current directory).
    /// Fails if it cannot.
    async fn resolve(&self, path: &Path) -> io::Result<PathBuf>;

    /// Answer from this sys's own state if it can; otherwise call `real` for
    /// the (already cleaned, absolute) path.
    async fn read_through(
        &self,
        path: &Path,
        real: &dyn RealRead,
    ) -> io::Result<Cow<'static, [u8]>>;
}

/// A [`ReadThrough`] with no state of its own: resolve against the sys's
/// current directory, then read from disk.
pub struct PlainReadThrough<S>(pub S);

#[async_trait]
impl<S> ReadThrough for PlainReadThrough<S>
where
    S: EnvCurrentDirAsync + Send + Sync,
{
    async fn resolve(&self, path: &Path) -> io::Result<PathBuf> {
        if path.is_absolute() {
            return Ok(path.to_path_buf());
        }
        Ok(self.0.env_current_dir_async().await?.join(path))
    }

    async fn read_through(
        &self,
        path: &Path,
        real: &dyn RealRead,
    ) -> io::Result<Cow<'static, [u8]>> {
        real.read(path).await
    }
}

/// Reads for confined scripts through the input view.
///
/// Order for every read: make the path absolute (the seam), refuse anything
/// that is not valid UTF-8, clean it lexically (`..` and `.` removed, never
/// resolved on disk), then:
///
/// - **inside the workspace root**: authorize, ask the seam's own state, and
///   on a miss read through a fresh `InputView` (symlinks rejected, limits
///   before allocation);
/// - **wholly outside the root** (`OutsideRoot`, and only that): refused if it
///   resolves back into the workspace (a best-effort alias check), otherwise
///   delegated to the broker unchanged;
/// - **anything else** that is not a valid workspace path: refused.
///
/// There is no fallback: an unsupported platform fails the read.
pub struct ContainedReader<S, T> {
    delegate: S,
    through: T,
    root: Option<PathBuf>,
    authorizer: Arc<dyn InputAuthorizer>,
    limits: InputLimits,
}

impl<S, T> ContainedReader<S, T> {
    /// `delegate` is the broker-wrapped sys (it serves paths outside the
    /// workspace), `authorizer` the same policy the broker uses, and `root` the
    /// canonical workspace root. With no root every read fails closed.
    pub fn new<A>(
        delegate: S,
        through: T,
        authorizer: A,
        root: Option<PathBuf>,
        limits: InputLimits,
    ) -> Self
    where
        A: CapabilityAuthorizer + 'static,
    {
        let adapter = CapabilityInputAuthorizer::new(
            authorizer,
            root.clone().unwrap_or_default(),
        );
        Self {
            delegate,
            through,
            root,
            authorizer: Arc::new(adapter),
            limits,
        }
    }
}

struct CancelOnDrop(Cancellation);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

struct ViewRead {
    root: PathBuf,
    authorizer: Arc<dyn InputAuthorizer>,
    limits: InputLimits,
}

#[async_trait]
impl RealRead for ViewRead {
    async fn read(&self, path: &Path) -> io::Result<Cow<'static, [u8]>> {
        // The seam keys its own state by the path it resolved; the view reads
        // the lexically cleaned form of it.
        let relative = WorkspacePath::from_os_path(
            &self.root,
            &omni_utils::path::clean(path),
        )
        .map_err(|_| invalid_path())?;
        let root = self.root.clone();
        let authorizer = Arc::clone(&self.authorizer);
        let limits = self.limits.clone();
        let cancellation = Cancellation::new();
        let _guard = CancelOnDrop(cancellation.clone());
        let read = tokio::task::spawn_blocking(move || {
            let view = InputView::open(
                &root,
                InputViewOptions {
                    authorizer,
                    limits,
                    symlinks: SymlinkPolicy::Reject,
                },
            )?;
            view.read(&relative, &cancellation)
        })
        .await
        .map_err(|error| {
            io::Error::other(format!("read task failed: {error}"))
        })?
        .map_err(to_io_error)?;
        match read {
            Read::Found(bytes) => Ok(Cow::Owned(bytes.to_vec())),
            Read::Missing => Err(io::Error::from(io::ErrorKind::NotFound)),
        }
    }
}

fn invalid_path() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "the path is not a valid workspace path",
    )
}

#[async_trait]
impl<S, T> BaseFsReadAsync for ContainedReader<S, T>
where
    S: BaseFsReadAsync + Send + Sync,
    T: ReadThrough,
{
    async fn base_fs_read_async(
        &self,
        path: &Path,
    ) -> io::Result<Cow<'static, [u8]>> {
        let Some(root) = self.root.as_ref() else {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "contained reads are enabled but the workspace root is unknown",
            ));
        };

        let absolute = self.through.resolve(path).await?;
        if !absolute.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the path could not be made absolute",
            ));
        }
        if absolute.to_str().is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the path is not valid UTF-8",
            ));
        }
        let cleaned = omni_utils::path::clean(&absolute);

        match WorkspacePath::from_os_path(root, &cleaned) {
            Ok(relative) => {
                match self.authorizer.authorize_read(&relative) {
                    Ok(AuthDecision::Allow) => {}
                    Ok(AuthDecision::Deny(reason)) => {
                        return Err(to_io_error(InputError::Denied(reason)));
                    }
                    Err(_) => {
                        return Err(to_io_error(InputError::PolicyUnavailable));
                    }
                }
                let real = ViewRead {
                    root: root.clone(),
                    authorizer: Arc::clone(&self.authorizer),
                    limits: self.limits.clone(),
                };
                // The seam gets the path as it resolved it (its own state is
                // keyed that way); the view cleans it again before reading. A
                // missing file or a buffered deletion is a plain `NotFound`,
                // so no absolute host path reaches the script.
                self.through.read_through(&absolute, &real).await.map_err(
                    |error| {
                        if error.kind() == io::ErrorKind::NotFound {
                            io::Error::from(io::ErrorKind::NotFound)
                        } else {
                            error
                        }
                    },
                )
            }
            Err(WorkspacePathError::OutsideRoot { .. }) => {
                // A path that looks outside the workspace may still reach it
                // through a symlink, either as the cleaned spelling or as the
                // OS would resolve the original (`link/..` follows the link
                // before going up). Refuse unless every resolved form is
                // provably outside; this is a best-effort, racy pre-check (a
                // link swapped after it is still subject to the broker's
                // policy check on the real path, and to the OS sandbox floor).
                let probes = [cleaned.clone(), absolute.clone()];
                let root = root.clone();
                let aliased = tokio::task::spawn_blocking(move || {
                    probes.iter().any(|probe| {
                        resolve_real_path(probe).is_some_and(|real| {
                            !matches!(
                                WorkspacePath::from_os_path(
                                    &root,
                                    &omni_utils::path::clean(real),
                                ),
                                Err(WorkspacePathError::OutsideRoot { .. })
                            )
                        })
                    })
                })
                .await
                .map_err(|error| {
                    io::Error::other(format!("alias check failed: {error}"))
                })?;
                if aliased {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "the path resolves into the workspace through an alias; \
                         use the workspace path",
                    ));
                }
                // Delegate the path that was judged, not the original
                // spelling, so the broker cannot resolve it differently.
                self.delegate.base_fs_read_async(&cleaned).await
            }
            Err(_) => Err(invalid_path()),
        }
    }
}
