use system_traits::{
    EnvCurrentDirAsync, FsCreateDirAllAsync, FsMetadataAsync, FsWriteAsync,
    auto_impl,
};

#[auto_impl]
pub trait JsRuntimeSys:
    FsWriteAsync
    + FsMetadataAsync
    + EnvCurrentDirAsync
    + FsCreateDirAllAsync
    + Clone
    + Send
    + Sync
{
}
