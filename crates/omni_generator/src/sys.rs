use system_traits::{FsReadAsync, FsWriteAsync, auto_impl};

#[auto_impl]
pub trait GeneratorSys:
    Clone + FsWriteAsync + FsReadAsync + Send + Sync + 'static
{
}
