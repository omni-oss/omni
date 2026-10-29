use system_traits::{
    EnvCurrentDir, EnvVar, EnvVars, FsCanonicalize, FsCopyAsync,
    FsCreateDirAllAsync, FsHardLinkAsync, FsMetadata, FsMetadataAsync, FsRead,
    FsReadAsync, FsWriteAsync, auto_impl,
};

#[auto_impl]
pub trait ContextSys:
    EnvCurrentDir
    + FsCreateDirAllAsync
    + FsWriteAsync
    + FsRead
    + FsReadAsync
    + FsMetadata
    + FsMetadataAsync
    + FsCanonicalize
    + Clone
    + EnvVar
    + EnvVars
    + FsHardLinkAsync
    + FsCopyAsync
    + Send
    + Sync
    + 'static
{
}
