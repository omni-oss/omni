use std::{fmt::Display, path::Path};

use super::Hash;
pub use omni_hasher::Hasher;

#[async_trait::async_trait]
pub trait ProjectDirHasher {
    type Error: Display;

    async fn hash<THasher: Hasher>(
        &self,
        project_name: &str,
        project_dir: &Path,
        include: &[&Path],
    ) -> Result<Hash<THasher>, Self::Error>;
}
