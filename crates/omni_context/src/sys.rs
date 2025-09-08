use system_traits::{
    EnvCurrentDir, EnvVar, EnvVars, FsCanonicalize, FsHardLinkAsync,
    FsMetadata, FsMetadataAsync, FsRead, FsReadAsync, auto_impl,
};

#[auto_impl]
pub trait ContextSys:
    EnvCurrentDir
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
