use std::path::Path;

use futures::future::try_join_all;
use omni_hasher::hash_file_in_path_async;
use rs_merkle::MerkleTree;
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use system_traits::{FsReadAsync, auto_impl};

use crate::hash::{Compat, Hasher};

#[auto_impl]
pub trait UtilSys: FsReadAsync + Send + Sync + Clone {}

pub async fn build_merkle_tree<THasher: Hasher>(
    paths: &[impl AsRef<Path>],
    sys: impl UtilSys,
) -> Result<MerkleTree<Compat<THasher>>, BuildMerkleTreeError> {
    let hashes =
        try_join_all(paths.iter().map(|p| {
            hash_file_in_path_async::<THasher>(p.as_ref(), sys.clone())
        }))
        .await?;

    let tree = MerkleTree::from_leaves(&hashes);

    Ok(tree)
}

#[derive(Debug, thiserror::Error)]
#[error("{inner}")]
pub struct BuildMerkleTreeError {
    inner: BuildMerkleTreeErrorInner,
    kind: BuildMerkleTreeErrorKind,
}

impl<T: Into<BuildMerkleTreeErrorInner>> From<T> for BuildMerkleTreeError {
    fn from(inner: T) -> Self {
        let error = inner.into();
        let kind = error.discriminant();
        Self { inner: error, kind }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(vis(pub), name(BuildMerkleTreeErrorKind))]
enum BuildMerkleTreeErrorInner {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Other(#[from] eyre::Report),

    #[error(transparent)]
    Hasher(#[from] omni_hasher::HasherError),
}
