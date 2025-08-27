use std::path::{Path, PathBuf};

use crate::{
    Hasher,
    project_dir_hasher::{Compat, ProjectDirHasher},
};

use super::utils;
use derive_builder::Builder;
use derive_new::new;
use omni_types::{OmniPath, Root, enum_map};
use path_clean::clean;
use rs_merkle::MerkleTree;
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use system_traits::impls::RealSys;

#[derive(Clone, Debug, new, Builder)]
pub struct RealDirHasher {
    #[new(default)]
    #[builder(setter(skip), default)]
    sys: RealSys,
    #[builder(setter(into))]
    index_dir: PathBuf,
    #[builder(setter(into))]
    workspace_root_dir: PathBuf,
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
        files: &[OmniPath],
    ) -> Result<MerkleTree<Compat<THasher>>, Self::Error> {
        let proj_dir = clean(project_dir);
        let ws_dir = clean(&self.workspace_root_dir);

        let bases = enum_map! {
            Root::Workspace => ws_dir.as_path(),
            Root::Project => proj_dir.as_path(),
        };

        let mut paths = vec![];

        for p in files {
            if p.is_rooted() {
                paths.push(p.clone());
            } else if p.unresolved_path().starts_with(&proj_dir) {
                paths.push(OmniPath::new_project_rooted(p.unresolved_path()));
            } else if p.unresolved_path().starts_with(&ws_dir) {
                paths.push(OmniPath::new_ws_rooted(p.unresolved_path()));
            } else {
                paths.push(OmniPath::new_project_rooted(p.unresolved_path()));
            }
        }

        // Sort from longest to shortest paths
        paths.sort_by(|a, b| b.cmp(a));

        // Build a merkle tree of the paths
        let tree = utils::build_merkle_tree::<THasher>(
            project_name,
            &bases,
            &paths,
            &self.index_dir,
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
    Builder(#[from] dir_walker::impls::RealGlobDirWalkerConfigBuilderError),

    #[error(transparent)]
    Ignore(#[from] dir_walker::impls::IgnoreError),

    #[error(transparent)]
    IgnoreRealDirWalker(#[from] dir_walker::impls::IgnoreRealDirWalkerError),

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
    use crate::blake3::Blake3Hasher;
    use system_traits::{
        FsCreateDirAllAsync, FsRename, FsWriteAsync, impls::RealSys,
    };

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
            .index_dir(root.join(".omni/index"))
            .workspace_root_dir(root)
            .build()
            .expect("failed to build hasher")
    }

    fn files() -> Vec<OmniPath> {
        vec![
            OmniPath::new("src/a-test.txt"),
            OmniPath::new("src/b-test.txt"),
        ]
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
                &files(),
            )
            .await
            .expect("failed to hash");

        let hash2 = hasher
            .hash::<Blake3Hasher>(
                "project",
                &tempdir.join("./project"),
                &files(),
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
                &files(),
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
                &files(),
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
                &files(),
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
                &files(),
            )
            .await
            .expect("failed to hash");

        let hash2 = hasher
            .hash::<Blake3Hasher>(
                "project",
                &tempdir.join("./project"),
                &files(),
            )
            .await
            .expect("failed to hash");

        let a_path = tempdir.join("./project/src/a-test.txt");
        let a_path_renamed = tempdir.join("./project/src/a-test-renamed.txt");

        // rename the project to simulate a move operation
        sys()
            .fs_rename(&a_path, &a_path_renamed)
            .expect("failed to rename");

        // the hash should be the same as hash1
        let hash3 = hasher
            .hash::<Blake3Hasher>(
                "project",
                &tempdir.join("./project"),
                &[
                    OmniPath::new("src/a-test-renamed.txt"),
                    OmniPath::new("src/b-test.txt"),
                ],
            )
            .await
            .expect("failed to hash");

        assert_eq!(hash1, hash2, "hashes should be equal");
        assert_ne!(hash1, hash3, "hashes should be different");
    }
}
