use system_traits::{
    BaseEnvSetCurrentDirAsync, BaseFsMetadataAsync, EnvCurrentDirAsync,
    EnvVars, FsAppendAsync, FsCopyAsync, FsCreateDirAllAsync, FsCreateDirAsync,
    FsGlobAsync, FsMetadataAsync, FsReadAsync, FsReadDirAsync,
    FsRemoveDirAllAsync, FsRemoveDirAsync, FsRemoveFileAsync, FsRenameAsync,
    FsWriteAsync, auto_impl,
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

/// Aggregate bound describing everything [`GeneratorSys`] provides plus the
/// ability to glob over files with pending writes ([`FsGlobAsync`]).
///
/// The [`FsGlobAsync`] / [`system_traits::BaseFsGlobAsync`] traits live in
/// `system_traits`. They are intentionally *not* part of [`GeneratorSys`]:
/// only overlay systems such as [`TransactionSys`] track pending writes, so the
/// capability is modelled as a separate trait and combined with
/// [`GeneratorSys`] here.
///
/// Action handlers that need to discover the files a generation has written so
/// far (e.g. `transform-many`) take a `GeneratorSysFull` instead of a plain
/// [`GeneratorSys`]. Every [`GeneratorSys`] that also implements
/// [`FsGlobAsync`] is a `GeneratorSysFull`.
///
/// [`TransactionSys`]: crate::TransactionSys
pub trait GeneratorSysFull: GeneratorSys + FsGlobAsync {}

impl<T> GeneratorSysFull for T
where
    T: GeneratorSys + FsGlobAsync,
    <T as BaseFsMetadataAsync>::Metadata: 'static,
{
}
