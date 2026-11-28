use system_traits::{
    FsCreateDirAllAsync, FsMetadataAsync, FsRemoveDirAllAsync, FsRenameAsync,
    auto_impl,
};

#[auto_impl]
pub trait GitUtilsSys:
    FsCreateDirAllAsync
    + FsMetadataAsync
    + FsRemoveDirAllAsync
    + FsRenameAsync
    + Send
    + Sync
{
}
