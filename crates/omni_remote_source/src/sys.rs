use omni_lockfile::LockfileSys;
use system_traits::{FsCreateDirAllAsync, FsRemoveDirAllAsync, auto_impl};

#[auto_impl]
pub trait RemoteSourceSys:
    Clone + LockfileSys + FsCreateDirAllAsync + FsRemoveDirAllAsync + 'static
{
}
