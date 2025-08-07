use std::path::Path;

use super::{super::Hash, utils};
use derive_builder::Builder;
use derive_new::new;
use dir_walker::{DirEntry, DirWalker, impls::RealGlobDirWalker};
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use system_traits::{FsCanonicalize, impls::RealSys};

use crate::hash::{DirHasher, Hasher};

#[derive(Clone, Debug, Default, new, Builder)]
pub struct RealDirHasher {
    #[new(default)]
    #[builder(setter(skip), default)]
    sys: RealSys,
    respect_standard_ignore_files: bool,
    custom_ignore_files: Vec<String>,
}

#[async_trait::async_trait]
impl DirHasher for RealDirHasher {
    type Error = RealDirHasherError;

    async fn hash<'a, 'b, THasher: Hasher>(
        &self,
        base_dir: &Path,
        paths: &'a [&'b Path],
    ) -> Result<Hash<THasher>, Self::Error>
    where
        'b: 'a,
    {
        let paths = paths
            .iter()
            .map(|p| p.to_string_lossy())
            .collect::<Vec<_>>();
        let dir_walker = RealGlobDirWalker::builder()
            .standard_filters(self.respect_standard_ignore_files)
            .custom_ignore_filenames(self.custom_ignore_files.clone())
            .include(paths.iter().map(|p| p as &str).collect::<Vec<_>>())?
            .build()?;

        let mut paths = vec![];
        let base_dir = self.sys.fs_canonicalize(base_dir)?;
        for p in dir_walker.walk_dir(&base_dir) {
            let p = self.sys.fs_canonicalize(base_dir.join(p?.path()))?;
            paths.push(p);
        }

        // Sort from longest to shortest paths
        paths.sort_by(|a, b| b.cmp(a));

        // Build a merkle tree of the paths
        let tree =
            utils::build_merkle_tree::<THasher>(&paths, self.sys.clone())
                .await?;

        let hash = tree.root().ok_or_else(|| eyre::eyre!("no root"))?;

        Ok(Hash::<THasher>::new(hash))
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{inner}")]
pub struct RealDirHasherError {
    inner: RealDirHasherErrorInner,
    kind: RealDirHasherErrorKind,
}

impl RealDirHasherError {
    pub fn kind(&self) -> RealDirHasherErrorKind {
        self.kind
    }
}

impl<T: Into<RealDirHasherErrorInner>> From<T> for RealDirHasherError {
    fn from(inner: T) -> Self {
        let error = inner.into();
        let kind = error.discriminant();
        Self { inner: error, kind }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(vis(pub), name(RealDirHasherErrorKind))]
enum RealDirHasherErrorInner {
    #[error(transparent)]
    Globset(#[from] dir_walker::impls::GlobsetError),

    #[error(transparent)]
    Builder(#[from] dir_walker::impls::RealGlobDirWalkerBuilderError),

    #[error(transparent)]
    Ignore(#[from] dir_walker::impls::IgnoreError),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    MerkleTree(#[from] eyre::Report),

    #[error(transparent)]
    Hasher(#[from] utils::BuildMerkleTreeError),
}
