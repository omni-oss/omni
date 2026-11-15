use system_traits::{
    FsMetadataAsync, FsOpen, FsReadAsync, FsRemoveFile, FsRename, FsWriteAsync,
    auto_impl,
};

#[auto_impl]
pub trait LockfileSys:
    FsMetadataAsync + FsReadAsync + FsWriteAsync + Send + Sync
{
}

/// Filesystem operations needed for staged, synced replacement publication.
#[auto_impl]
pub trait LockfileSaveSys:
    LockfileSys + FsOpen + FsRename + FsRemoveFile + Clone + 'static
{
}
