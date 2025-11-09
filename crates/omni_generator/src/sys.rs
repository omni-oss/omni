use system_traits::{
    FsCreateDirAllAsync, FsMetadataAsync, FsReadAsync, FsRemoveDirAllAsync,
    FsRemoveDirAsync, FsRemoveFileAsync, FsWriteAsync, auto_impl,
};

#[auto_impl]
pub trait GeneratorSys:
    Clone
    + FsWriteAsync
    + FsReadAsync
    + Send
    + Sync
    + FsMetadataAsync
    + FsCreateDirAllAsync
    + FsRemoveDirAllAsync
    + FsRemoveDirAsync
    + FsRemoveFileAsync
    + 'static
{
}
