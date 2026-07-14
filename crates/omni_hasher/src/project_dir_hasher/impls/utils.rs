use std::{
    collections::HashMap,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use byteorder::{ByteOrder, LittleEndian};
use futures::future::try_join_all;
use omni_types::{OmniPath, Root, RootMap};
use omni_utils::path::path_safe;
use rs_merkle::MerkleTree;
use serde::{Deserialize, Serialize};
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use system_traits::{
    FsCreateDirAllAsync, FsMetadataAsync, FsMetadataValue, FsReadAsync,
    FsRenameAsync, FsWriteAsync, auto_impl,
};

use crate::{
    Hasher, hash_file_in_path_async,
    project_dir_hasher::{Compat, Hash},
};

#[auto_impl]
pub trait UtilSys:
    FsReadAsync
    + FsWriteAsync
    + FsRenameAsync
    + FsCreateDirAllAsync
    + Send
    + Sync
    + Clone
    + FsMetadataAsync
{
}

type Timestamp = [u8; 16];

/// A cheap change-detection stamp for a file. We deliberately combine the
/// modification time with the file size: mtime alone is unreliable because its
/// on-disk resolution is coarse (and captured here only to the millisecond), so
/// two writes that land within the same tick are indistinguishable by mtime.
/// Including the size catches any content change that also changes the length,
/// which makes cache invalidation robust against that timestamp race.
struct FileStamp {
    mtime: Timestamp,
    size: u64,
}

async fn file_stamp(
    path: &Path,
    sys: impl UtilSys,
) -> Result<FileStamp, BuildMerkleTreeError> {
    let metadata = sys.fs_metadata_async(path).await?;

    let mtime = metadata.modified()?;
    let mtime = mtime.duration_since(UNIX_EPOCH)?.as_millis();

    let mut timestamp = [0; 16];
    LittleEndian::write_u128(&mut timestamp, mtime);

    Ok(FileStamp {
        mtime: timestamp,
        size: metadata.len(),
    })
}

#[derive(Serialize, Deserialize, Clone, Eq)]
struct FileEntry<THasher: Hasher> {
    path: OmniPath,
    hash: THasher::Hash,
    mtime: Timestamp,
    size: u64,
}

impl<THasher: Hasher> PartialEq for FileEntry<THasher> {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
            && self.hash == other.hash
            && self.mtime == other.mtime
            && self.size == other.size
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
                    log::error!("Failed to read partial hashes file {partial_hashes_file:?}: {e}");
                })?;

        // The cache file is written by potentially many concurrent tasks. A
        // partially-written or otherwise corrupt file should never be fatal:
        // we simply treat it as an empty cache and recompute, since these are
        // only optimization hints used to avoid rewriting unchanged entries.
        match bincode_next::serde::borrow_decode_from_slice::<
            Vec<FileEntry<THasher>>,
            _,
        >(&bytes, bincode_next::config::standard())
        {
            Ok((file_entries, _size)) => {
                file_entries_by_path.extend(
                    file_entries.iter().cloned().map(|e| (e.path.clone(), e)),
                );

                file_entries
            }
            Err(e) => {
                log::warn!(
                    "Ignoring corrupt partial hashes file {partial_hashes_file:?}: {e}"
                );
                vec![]
            }
        }
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
            let FileStamp { mtime, size } =
                file_stamp(&abs_path, sys.clone()).await?;

            let hash = if let Some(entry) = file_entries_by_path.get(&path)
                && entry.mtime == mtime
                && entry.size == size
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
                    size,
                    path,
                }
            };

            Ok::<_, BuildMerkleTreeError>(hash)
        });
    }

    let new_hashes = try_join_all(tasks).await?;

    if new_hashes != file_entries {
        let bytes = bincode_next::serde::encode_to_vec(
            &new_hashes,
            bincode_next::config::standard(),
        )?;

        // Write atomically: write to a unique temp file and rename it over the
        // target. This guarantees concurrent readers always observe either the
        // previous complete file or the new complete file, never a partially
        // written one (which would fail to decode).
        let tmp_file = project_dir_path
            .join(format!("partial-hashes.bin.{}.tmp", unique_tmp_suffix()));

        sys.fs_write_async(&tmp_file, &bytes)
            .await
            .inspect_err(|e| {
                log::error!(
                    "Failed to write partial hashes temp file {tmp_file:?}: {e}"
                );
            })?;

        sys.fs_rename_async(&tmp_file, &partial_hashes_file).await.inspect_err(|e| {
            log::error!("Failed to rename partial hashes file into place {partial_hashes_file:?}: {e}");
        })?;
    }

    let mut tree = MerkleTree::new();

    tree.append(&mut new_hashes.iter().map(|h| h.hash).collect::<Vec<_>>());

    Ok(tree)
}

/// Generates a process-and-task-unique suffix for temp files so that
/// concurrent writers never clobber each other's in-progress files.
fn unique_tmp_suffix() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);

    format!("{}-{nanos}-{count}", std::process::id())
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
    Decode(#[from] bincode_next::error::DecodeError),

    #[error(transparent)]
    Encode(#[from] bincode_next::error::EncodeError),
}
