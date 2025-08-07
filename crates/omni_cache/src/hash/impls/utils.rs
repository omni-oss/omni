use std::{collections::HashMap, path::Path, time::UNIX_EPOCH};

use futures::future::try_join_all;
use rs_merkle::MerkleTree;
use serde::{Deserialize, Serialize};
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use system_traits::{
    FsCreateDirAllAsync, FsMetadataAsync, FsMetadataValue, FsReadAsync,
    FsWriteAsync, auto_impl,
};

use crate::hash::{Compat, Hasher};

#[auto_impl]
pub trait UtilSys:
    FsReadAsync
    + FsWriteAsync
    + FsCreateDirAllAsync
    + Send
    + Sync
    + Clone
    + FsMetadataAsync
{
}

fn relpath<'a>(path: &'a Path, base: &Path) -> &'a Path {
    path.strip_prefix(base)
        .expect("path is not a child of base")
}

async fn mtime(
    path: &Path,
    sys: impl UtilSys,
) -> Result<u128, BuildMerkleTreeError> {
    let mtime = sys.fs_metadata_async(path).await?.modified()?;

    Ok(mtime.duration_since(UNIX_EPOCH)?.as_millis())
}

#[derive(Serialize, Deserialize, Clone, Eq)]
struct FileEntry<'a, THasher: Hasher> {
    #[serde(borrow)]
    path: &'a Path,
    hash: THasher::Hash,
    mtime: u128,
}

impl<'a, THasher: Hasher> PartialEq for FileEntry<'a, THasher> {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
            && self.hash == other.hash
            && self.mtime == other.mtime
    }
}

pub async fn build_merkle_tree<THasher: Hasher>(
    project_name: &str,
    project_dir: &Path,
    paths: &[impl AsRef<Path>],
    index_dir: &Path,
    sys: impl UtilSys,
) -> Result<MerkleTree<Compat<THasher>>, BuildMerkleTreeError> {
    let project_name_hash = THasher::hash(project_name.as_bytes());
    let project_dir_name =
        bs58::encode(project_name_hash.as_ref()).into_string();
    let project_dir_path = index_dir.join(project_dir_name);

    let mut file_entries_by_path = HashMap::<&Path, FileEntry<THasher>>::new();

    if !sys.fs_exists_async(&project_dir_path).await? {
        sys.fs_create_dir_all_async(&project_dir_path).await?;
    }

    let partial_hashes_file = project_dir_path.join("partial-hashes.bin");

    let bytes;

    let file_entries = if sys.fs_exists_async(&partial_hashes_file).await? {
        bytes = sys.fs_read_async(&partial_hashes_file).await?;

        let (file_entries, _size): (Vec<FileEntry<THasher>>, usize) =
            bincode::serde::borrow_decode_from_slice(
                &bytes,
                bincode::config::standard(),
            )?;

        file_entries_by_path
            .extend(file_entries.iter().cloned().map(|e| (e.path, e)));

        file_entries
    } else {
        vec![]
    };

    let mut tasks = vec![];

    for path in paths.iter().map(AsRef::as_ref) {
        tasks.push(async {
            let rel = relpath(path, project_dir);
            let mtime = mtime(path, sys.clone()).await?;

            let hash = if let Some(entry) = file_entries_by_path.get(rel)
                && entry.mtime == mtime
            {
                entry.clone()
            } else {
                let hash = omni_hasher::hash_file_in_path_async::<THasher>(
                    path,
                    sys.clone(),
                )
                .await?;

                FileEntry {
                    hash,
                    mtime,
                    path: rel,
                }
            };

            Ok::<_, BuildMerkleTreeError>(hash)
        });
    }

    let new_hashes = try_join_all(tasks).await?;

    if new_hashes != file_entries {
        let bytes = bincode::serde::encode_to_vec(
            &new_hashes,
            bincode::config::standard(),
        )?;

        sys.fs_write_async(&partial_hashes_file, &bytes).await?;
    }

    let tree = MerkleTree::from_leaves(
        &new_hashes.iter().map(|h| h.hash).collect::<Vec<_>>(),
    );

    Ok(tree)
}

#[derive(Debug, thiserror::Error)]
#[error("{inner}")]
pub struct BuildMerkleTreeError {
    kind: BuildMerkleTreeErrorKind,
    inner: BuildMerkleTreeErrorInner,
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

    #[error(transparent)]
    SystemTime(#[from] std::time::SystemTimeError),

    #[error(transparent)]
    Decode(#[from] bincode::error::DecodeError),

    #[error(transparent)]
    Encode(#[from] bincode::error::EncodeError),
}
