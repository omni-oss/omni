use system_traits::{FsMetadataAsync, FsReadAsync, FsWriteAsync, auto_impl};

#[auto_impl]
pub trait LockfileSys:
    FsMetadataAsync + FsReadAsync + FsWriteAsync + Send + Sync
{
}
