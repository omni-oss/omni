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
