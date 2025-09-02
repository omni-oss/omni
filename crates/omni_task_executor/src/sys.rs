use omni_context::ContextSys;
use system_traits::FsCreateDirAllAsync;

#[system_traits::auto_impl]
pub trait TaskExecutorSys: ContextSys + FsCreateDirAllAsync {}
