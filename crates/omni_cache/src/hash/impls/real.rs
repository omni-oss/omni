use std::path::{Path, PathBuf};

use super::utils;
use derive_builder::Builder;
use derive_new::new;
use dir_walker::{DirEntry, DirWalker, impls::RealGlobDirWalker};
use rs_merkle::MerkleTree;
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use system_traits::{FsCanonicalize, FsMetadataAsync, impls::RealSys};

use crate::hash::{Compat, Hasher, ProjectDirHasher};

#[derive(Clone, Debug, new, Builder)]
pub struct RealDirHasher {
    #[new(default)]
    #[builder(setter(skip), default)]
    sys: RealSys,
    standard_ignore_files: bool,
    custom_ignore_files: Vec<String>,
    dir: PathBuf,
}

impl RealDirHasher {
    pub fn builder() -> RealDirHasherBuilder {
        RealDirHasherBuilder::default()
    }
}

#[async_trait::async_trait]
impl ProjectDirHasher for RealDirHasher {
    type Error = RealDirHasherError;

    async fn hash_tree<THasher: Hasher>(
        &self,
        project_name: &str,
        project_dir: &Path,
        files: &[&Path],
    ) -> Result<MerkleTree<Compat<THasher>>, Self::Error> {
        let dir_walker = RealGlobDirWalker::builder()
            .standard_filters(self.standard_ignore_files)
            .custom_ignore_filenames(self.custom_ignore_files.clone())
            .include(files.iter().map(|p| p.to_path_buf()).collect::<Vec<_>>())
            .build()?;

        let mut paths = vec![];
        let base_dir = self.sys.fs_canonicalize(project_dir)?;
        for p in dir_walker.walk_dir(&base_dir) {
            let p = self.sys.fs_canonicalize(base_dir.join(p?.path()))?;

            if self.sys.fs_is_file_async(&p).await? {
                paths.push(p);
            }
        }

        // Sort from longest to shortest paths
        paths.sort_by(|a, b| b.cmp(a));

        // Build a merkle tree of the paths
        let tree = utils::build_merkle_tree::<THasher>(
            project_name,
            project_dir,
            &paths,
            &self.dir,
            self.sys.clone(),
        )
        .await?;

        Ok(tree)
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
        let inner = inner.into();
        let kind = inner.discriminant();
        Self { inner, kind }
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
    BuildMerkleTree(#[from] utils::BuildMerkleTreeError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use omni_hasher::impls::Blake3Hasher;
    use system_traits::{FsCreateDirAllAsync, FsWriteAsync, impls::RealSys};

    use tempfile::tempdir;

    #[inline(always)]
    fn sys() -> RealSys {
        RealSys
    }

    async fn test_fixture() -> tempfile::TempDir {
        let sys = sys();
        let dir = tempdir().expect("failed to create temp dir");

        let root = dir.path();

        sys.fs_create_dir_all_async(root.join("./project/src"))
            .await
            .expect("failed to create test dir");

        sys.fs_create_dir_all_async(root.join("./.omni"))
            .await
            .expect("failed to index dir");

        sys.fs_write_async(
            root.join("./project/src/a-test.txt"),
            include_str!("../../../test_fixtures/test.txt"),
        )
        .await
        .expect("failed to write test file");

        sys.fs_write_async(
            root.join("./project/src/b-test.txt"),
            include_str!("../../../test_fixtures/test.txt"),
        )
        .await
        .expect("failed to write test file");

        dir
    }

    fn dir_hasher(root: &Path) -> RealDirHasher {
        RealDirHasher::builder()
            .custom_ignore_files(vec![".omniignore".to_string()])
            .standard_ignore_files(true)
            .dir(root.join(".omni/index"))
            .build()
            .expect("failed to build hasher")
    }

    #[tokio::test]
    async fn test_hash_unchanged() {
        let dir = test_fixture().await;
        let tempdir = dir.path();

        let hasher = dir_hasher(tempdir);

        let hash1 = hasher
            .hash::<Blake3Hasher>(
                "project",
                &tempdir.join("./project"),
                &[Path::new("./src/**/*.txt")],
            )
            .await
            .expect("failed to hash");

        let hash2 = hasher
            .hash::<Blake3Hasher>(
                "project",
                &tempdir.join("./project"),
                &[Path::new("./src/**/*.txt")],
            )
            .await
            .expect("failed to hash");

        assert_eq!(hash1, hash2, "hashes should be equal");
    }

    #[tokio::test]
    async fn test_hash_changed() {
        let dir = test_fixture().await;
        let tempdir = dir.path();

        let hasher = dir_hasher(tempdir);
        let sys = sys();

        let hash1 = hasher
            .hash::<Blake3Hasher>(
                "project",
                &tempdir.join("./project"),
                &[Path::new("./src/**/*.txt")],
            )
            .await
            .expect("failed to hash");
        let a_path = tempdir.join("./project/src/a-test.txt");

        // modify the file
        sys.fs_write_async(&a_path, "changed file content")
            .await
            .expect("failed to write test file");

        // the hash should be different
        let hash2 = hasher
            .hash::<Blake3Hasher>(
                "project",
                &tempdir.join("./project"),
                &[Path::new("./src/**/*.txt")],
            )
            .await
            .expect("failed to hash");

        // revert the file content
        sys.fs_write_async(
            &a_path,
            include_str!("../../../test_fixtures/test.txt"),
        )
        .await
        .expect("failed to write test file");

        // the hash should be the same as hash1
        let hash3 = hasher
            .hash::<Blake3Hasher>(
                "project",
                &tempdir.join("./project"),
                &[Path::new("./src/**/*.txt")],
            )
            .await
            .expect("failed to hash");

        assert_ne!(hash1, hash2, "hashes should be different");
        assert_eq!(hash1, hash3, "hashes should be equal");
    }

    #[tokio::test]
    async fn test_renaming_a_file_should_invalidate_cache() {
        let dir = test_fixture().await;
        let tempdir = dir.path();
        let hasher = dir_hasher(tempdir);

        let hash1 = hasher
            .hash::<Blake3Hasher>(
                "project",
                &tempdir.join("./project"),
                &[Path::new("./src/**/*.txt")],
            )
            .await
            .expect("failed to hash");

        let hash2 = hasher
            .hash::<Blake3Hasher>(
                "project",
                &tempdir.join("./project"),
                &[Path::new("./src/**/*.txt")],
            )
            .await
            .expect("failed to hash");

        let a_path = tempdir.join("./project/src/a-test.txt");
        let a_path_renamed = tempdir.join("./project/src/a-test-renamed.txt");

        // rename the project to simulate a move operation
        tokio::fs::rename(&a_path, &a_path_renamed)
            .await
            .expect("failed to rename");

        // the hash should be the same as hash1
        let hash3 = hasher
            .hash::<Blake3Hasher>(
                "project",
                &tempdir.join("./project"),
                &[Path::new("./src/**/*.txt")],
            )
            .await
            .expect("failed to hash");

        assert_eq!(hash1, hash2, "hashes should be equal");
        assert_ne!(hash1, hash3, "hashes should be different");
    }
}
