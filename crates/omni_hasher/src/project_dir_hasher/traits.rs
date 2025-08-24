use std::{fmt::Display, path::Path};

use crate::{Hasher, project_dir_hasher::Compat};

use super::Hash;
use omni_types::OmniPath;
pub use rs_merkle::MerkleTree as HashTree;

#[async_trait::async_trait]
pub trait ProjectDirHasher {
    type Error: Display;

    async fn hash<THasher: Hasher>(
        &self,
        project_name: &str,
        project_dir: &Path,
        files: &[OmniPath],
    ) -> Result<Hash<THasher>, Self::Error> {
        let mut tree = self
            .hash_tree::<THasher>(project_name, project_dir, files)
            .await?;
        tree.commit();
        let hash = tree.root().expect("no root");

        Ok(Hash::<THasher>::new(hash))
    }

    async fn hash_tree<THasher: Hasher>(
        &self,
        project_name: &str,
        project_dir: &Path,
        files: &[OmniPath],
    ) -> Result<HashTree<Compat<THasher>>, Self::Error>;
}
