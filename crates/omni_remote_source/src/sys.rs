use omni_lockfile::LockfileSaveSys;
use system_traits::{
    FsCanonicalizeAsync, FsCreateDirAllAsync, FsReadDirAsync,
    FsRemoveDirAllAsync, FsRemoveFileAsync, FsRenameAsync, auto_impl,
};

#[auto_impl]
pub trait RemoteSourceSys:
    Clone
    + LockfileSaveSys
    + FsCanonicalizeAsync
    + FsCreateDirAllAsync
    + FsRemoveDirAllAsync
    + FsRemoveFileAsync
    + FsReadDirAsync
    + FsRenameAsync
    + 'static
{
}
