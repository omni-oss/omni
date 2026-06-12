use std::{
    io,
    path::{Path, PathBuf},
};

use system_traits::{
    BaseEnvSetCurrentDirAsync, BaseFsMetadataAsync, EnvCurrentDirAsync,
    EnvVars, FsAppendAsync, FsCopyAsync, FsCreateDirAllAsync, FsCreateDirAsync,
    FsMetadataAsync, FsReadAsync, FsReadDirAsync, FsRemoveDirAllAsync,
    FsRemoveDirAsync, FsRemoveFileAsync, FsRenameAsync, FsWriteAsync,
    auto_impl,
};

/// Aggregate bound describing everything a generator system handle must
/// provide: the full set of asynchronous file-system operations plus the
/// process/environment operations, along with `Clone` so handles can be
/// cheaply duplicated (e.g. to wrap them in a [`TransactionSys`]).
///
/// This is implemented automatically for any type that satisfies all of the
/// underlying `*_async` traits, so you should never need to implement it
/// manually.
///
/// [`TransactionSys`]: crate::TransactionSys
#[auto_impl]
pub trait GeneratorSys:
    Clone
    + Send
    + Sync
    + 'static
    + FsReadAsync
    + FsWriteAsync
    + FsAppendAsync
    + FsMetadataAsync
    + FsCreateDirAsync
    + FsCreateDirAllAsync
    + FsReadDirAsync
    + FsRemoveFileAsync
    + FsRemoveDirAsync
    + FsRemoveDirAllAsync
    + FsRenameAsync
    + FsCopyAsync
    + EnvCurrentDirAsync
    + BaseEnvSetCurrentDirAsync
    + EnvVars
where
    <Self as BaseFsMetadataAsync>::Metadata: 'static,
{
}

/// Matches glob patterns against the set of files that currently have pending
/// (uncommitted) writes in the system.
///
/// This is the "base" trait that implementors provide; prefer the ergonomic
/// [`FsGlobAsync`] for calling it. It is intentionally *not* part of
/// [`GeneratorSys`]: only overlay systems such as [`TransactionSys`] track
/// pending writes, so the capability is modelled as a separate trait and
/// combined with [`GeneratorSys`] in [`GeneratorSysFull`].
///
/// [`TransactionSys`]: crate::TransactionSys
#[async_trait::async_trait]
pub trait BaseFsGlobAsync {
    /// Returns every file with a pending in-memory write whose path matches one
    /// of `patterns`, with the patterns anchored under `root_dir`.
    ///
    /// A pattern prefixed with `!` is treated as an exclusion.
    #[doc(hidden)]
    async fn base_fs_glob_async(
        &self,
        root_dir: &Path,
        patterns: &[&str],
    ) -> io::Result<Vec<PathBuf>>;
}

/// Ergonomic wrapper over [`BaseFsGlobAsync`] that accepts any path-like
/// `root_dir` and any slice of string-like `patterns`.
#[async_trait::async_trait]
pub trait FsGlobAsync: BaseFsGlobAsync {
    async fn fs_glob_async<P: AsRef<str> + Sync>(
        &self,
        root_dir: impl AsRef<Path> + Send,
        patterns: &[P],
    ) -> io::Result<Vec<PathBuf>> {
        let patterns: Vec<&str> = patterns.iter().map(|p| p.as_ref()).collect();
        self.base_fs_glob_async(root_dir.as_ref(), &patterns).await
    }
}

impl<T: BaseFsGlobAsync> FsGlobAsync for T {}

/// Aggregate bound describing everything [`GeneratorSys`] provides plus the
/// ability to glob over files with pending writes ([`FsGlobAsync`]).
///
/// Action handlers that need to discover the files a generation has written so
/// far (e.g. `transform-many`) take a `GeneratorSysFull` instead of a plain
/// [`GeneratorSys`]. Every [`GeneratorSys`] that also implements
/// [`BaseFsGlobAsync`] is a `GeneratorSysFull`.
pub trait GeneratorSysFull: GeneratorSys + FsGlobAsync {}

impl<T> GeneratorSysFull for T
where
    T: GeneratorSys + FsGlobAsync,
    <T as BaseFsMetadataAsync>::Metadata: 'static,
{
}
