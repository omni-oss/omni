use system_traits::{FsWriteAsync, auto_impl};

#[auto_impl]
pub trait GeneratorSys: Clone + FsWriteAsync {}
