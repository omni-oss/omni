use omni_lockfile::LockfileSys;
use system_traits::{
    FsCreateDirAllAsync, FsReadDirAsync, FsRemoveDirAllAsync, FsRenameAsync,
    auto_impl,
};

#[auto_impl]
pub trait RemoteSourceSys:
    Clone
    + LockfileSys
    + FsCreateDirAllAsync
    + FsRemoveDirAllAsync
    + FsReadDirAsync
    + FsRenameAsync
    + 'static
{
}
