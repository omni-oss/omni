use omni_lockfile::LockfileSaveSys;
use system_traits::{
    FsCreateDirAllAsync, FsReadDirAsync, FsRemoveDirAllAsync,
    FsRemoveFileAsync, FsRenameAsync, auto_impl,
};

#[auto_impl]
pub trait RemoteSourceSys:
    Clone
    + LockfileSaveSys
    + FsCreateDirAllAsync
    + FsRemoveDirAllAsync
    + FsRemoveFileAsync
    + FsReadDirAsync
    + FsRenameAsync
    + 'static
{
}
