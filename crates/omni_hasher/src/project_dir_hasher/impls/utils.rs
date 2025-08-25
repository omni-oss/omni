use std::{collections::HashMap, path::Path, time::UNIX_EPOCH};

use byteorder::{ByteOrder, LittleEndian};
use futures::future::try_join_all;
use omni_types::{OmniPath, Root, RootMap};
use omni_utils::path::path_safe;
use rs_merkle::MerkleTree;
use serde::{Deserialize, Serialize};
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use system_traits::{
    FsCreateDirAllAsync, FsMetadataAsync, FsMetadataValue, FsReadAsync,
    FsWriteAsync, auto_impl,
};

use crate::{
    Hasher, hash_file_in_path_async,
    project_dir_hasher::{Compat, Hash},
};

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

type Timestamp = [u8; 16];

async fn mtime(
    path: &Path,
    sys: impl UtilSys,
) -> Result<Timestamp, BuildMerkleTreeError> {
    let mtime = sys.fs_metadata_async(path).await?.modified()?;
    let mtime = mtime.duration_since(UNIX_EPOCH)?.as_millis();

    let mut timestamp = [0; 16];

    LittleEndian::write_u128(&mut timestamp, mtime);

    Ok(timestamp)
}

#[derive(Serialize, Deserialize, Clone, Eq)]
struct FileEntry<THasher: Hasher> {
    path: OmniPath,
    hash: THasher::Hash,
    mtime: Timestamp,
}

impl<THasher: Hasher> PartialEq for FileEntry<THasher> {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
            && self.hash == other.hash
            && self.mtime == other.mtime
    }
}

pub async fn build_merkle_tree<THasher: Hasher>(
    project_name: &str,
    root_map: &RootMap<'_>,
    paths: &[OmniPath],
    index_dir: &Path,
    sys: impl UtilSys,
) -> Result<MerkleTree<Compat<THasher>>, BuildMerkleTreeError> {
    let project_dir_name = path_safe(project_name);
    let project_dir_path = index_dir.join(project_dir_name);

    let mut file_entries_by_path =
        HashMap::<OmniPath, FileEntry<THasher>>::new();

    if !sys.fs_exists_async(&project_dir_path).await? {
        sys.fs_create_dir_all_async(&project_dir_path).await?;
    }

    let partial_hashes_file = project_dir_path.join("partial-hashes.bin");

    let bytes;

    let file_entries = if sys.fs_exists_async(&partial_hashes_file).await? {
        bytes =
            sys.fs_read_async(&partial_hashes_file)
                .await
                .inspect_err(|e| {
                    trace::error!("failed to read partial hashes file {partial_hashes_file:?}: {e}");
                })?;

        let (file_entries, _size): (Vec<FileEntry<THasher>>, usize) =
            bincode::serde::borrow_decode_from_slice(
                &bytes,
                bincode::config::standard(),
            )?;

        file_entries_by_path
            .extend(file_entries.iter().cloned().map(|e| (e.path.clone(), e)));

        file_entries
    } else {
        vec![]
    };

    let mut tasks = vec![];

    for path in paths {
        tasks.push(async {
            let abs_path = path.resolve(root_map);
            let abs_path = if abs_path.is_relative() {
                root_map[Root::Project].join(abs_path)
            } else {
                abs_path.to_path_buf()
            };
            let path = path.clone();
            let mtime = mtime(&abs_path, sys.clone()).await?;

            let hash = if let Some(entry) = file_entries_by_path.get(&path)
                && entry.mtime == mtime
                && false
            {
                entry.clone()
            } else {
                let path_cache = THasher::hash(path.to_string().as_bytes());
                let content_hash =
                    hash_file_in_path_async::<THasher>(&abs_path, sys.clone())
                        .await?;
                let mut hash = Hash::<THasher>::new(path_cache);
                hash.combine_in_place(content_hash);

                FileEntry {
                    hash: hash.to_inner(),
                    mtime,
                    path,
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

        sys.fs_write_async(&partial_hashes_file, &bytes).await.inspect_err(|e| {
            trace::error!("failed to write partial hashes file {partial_hashes_file:?}: {e}");
        })?;
    }

    let mut tree = MerkleTree::new();

    tree.append(&mut new_hashes.iter().map(|h| h.hash).collect::<Vec<_>>());

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
    Hasher(#[from] crate::HasherError),

    #[error(transparent)]
    SystemTime(#[from] std::time::SystemTimeError),

    #[error(transparent)]
    Decode(#[from] bincode::error::DecodeError),

    #[error(transparent)]
    Encode(#[from] bincode::error::EncodeError),
}
