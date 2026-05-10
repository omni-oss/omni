use system_traits::{FsCreateDirAllAsync, auto_impl};

#[auto_impl]
pub trait GitUtilsSys: FsCreateDirAllAsync + Send + Sync {}
