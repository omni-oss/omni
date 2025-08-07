use std::{fmt::Display, path::Path};

use super::Hash;
pub use omni_hasher::Hasher;

#[async_trait::async_trait]
pub trait DirHasher {
    type Error: Display;

    async fn hash<'a, 'b, THasher: Hasher>(
        &self,
        base_dir: &Path,
        paths: &'a [&'b Path],
    ) -> Result<Hash<THasher>, Self::Error>
    where
        'b: 'a;
}
