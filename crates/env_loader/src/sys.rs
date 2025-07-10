use system_traits::{
    EnvCurrentDir, FsCanonicalize, FsMetadata, FsRead, auto_impl,
};

#[auto_impl]
pub trait EnvLoaderSys:
    EnvCurrentDir + FsRead + FsMetadata + FsCanonicalize
{
}
