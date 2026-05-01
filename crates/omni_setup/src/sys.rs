use system_traits::{
    FsCreateDirAllAsync, FsMetadataAsync, FsRead, FsReadAsync, FsWriteAsync,
    auto_impl,
};

#[auto_impl]
pub trait SetupSys:
    FsMetadataAsync
    + FsCreateDirAllAsync
    + FsWriteAsync
    + FsReadAsync
    + FsRead
    + Send
    + Sync
{
}
