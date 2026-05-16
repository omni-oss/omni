use omni_lockfile::LockfileSys;
use system_traits::{FsCreateDirAllAsync, auto_impl};

#[auto_impl]
pub trait RemoteSourceSys: LockfileSys + FsCreateDirAllAsync {}
