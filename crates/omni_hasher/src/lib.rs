mod error;
mod hasher;
pub mod project_dir_hasher;

pub use error::*;
pub use hasher::*;
use std::path::Path;
use system_traits::{FsRead, FsReadAsync};

#[inline(always)]
pub async fn hash_file_in_path_async<THasher: Hasher>(
    path: &Path,
    sys: impl FsReadAsync + Sync,
) -> Result<THasher::Hash, HasherError> {
    let file = sys.fs_read_async(path).await?;

    hash_bytes::<THasher>(&file)
}

#[inline(always)]
pub fn hash_file_in_path<THasher: Hasher>(
    path: &Path,
    sys: impl FsRead,
) -> Result<THasher::Hash, HasherError> {
    let file = sys.fs_read(path)?;

    hash_bytes::<THasher>(&file)
}

#[inline(always)]
pub fn hash_bytes<THasher: Hasher>(
    bytes: &[u8],
) -> Result<THasher::Hash, HasherError> {
    Ok(THasher::hash(bytes))
}

pub mod blake3 {
    use super::*;
    use hasher::Hasher;
    pub use hasher::impls::Blake3Hasher;

    #[inline(always)]
    pub async fn hash_file_in_path_async(
        path: &Path,
        sys: impl FsReadAsync + Sync,
    ) -> Result<<Blake3Hasher as Hasher>::Hash, HasherError> {
        let file = sys.fs_read_async(path).await?;

        hash_bytes(&file)
    }

    #[inline(always)]
    pub fn hash_file_in_path(
        path: &Path,
        sys: impl FsRead,
    ) -> Result<<Blake3Hasher as Hasher>::Hash, HasherError> {
        let file = sys.fs_read(path)?;

        hash_bytes(&file)
    }

    #[inline(always)]
    pub fn hash_bytes(
        bytes: &[u8],
    ) -> Result<<Blake3Hasher as Hasher>::Hash, HasherError> {
        super::hash_bytes::<Blake3Hasher>(bytes)
    }
}

pub mod default {
    pub use crate::hasher::impls::Blake3Hasher as DefaultHasher;

    pub use super::blake3::*;
}
