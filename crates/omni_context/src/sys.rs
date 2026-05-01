use system_traits::{
    EnvCurrentDir, EnvVar, EnvVars, FsCanonicalize, FsCreateDirAllAsync,
    FsHardLinkAsync, FsMetadata, FsMetadataAsync, FsRead, FsReadAsync,
    FsWriteAsync, auto_impl,
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
    + Send
    + Sync
    + 'static
{
}
