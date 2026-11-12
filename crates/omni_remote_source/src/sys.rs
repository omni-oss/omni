use omni_lockfile::LockfileSys;
use system_traits::{
    FsCreateDirAllAsync, FsReadDirAsync, FsRemoveDirAllAsync,
    FsRemoveFileAsync, FsRenameAsync, auto_impl,
};

#[auto_impl]
pub trait RemoteSourceSys:
    Clone
    + LockfileSys
    + FsCreateDirAllAsync
    + FsRemoveDirAllAsync
    + FsRemoveFileAsync
    + FsReadDirAsync
    + FsRenameAsync
    + 'static
{
}
