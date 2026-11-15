#![allow(clippy::redundant_field_names)]

use std::{collections::BTreeSet, path::PathBuf};

use async_trait::async_trait;
use bytes::Bytes;
use bytesize::ByteSize;
use derive_new::new;
use tokio::{
    fs::File,
    io::{AsyncReadExt as _, AsyncSeekExt as _, AsyncWriteExt as _},
};
use tokio_stream::StreamExt;
use tokio_util::io::ReaderStream;

use omni_remote_cache_storage_core::{
    ByteRange, Container, ContainerPrefix, ListItem, PrefixPage, ReadStream,
    RemoteCacheStorageBackend, SaveOutcome, WriteStream, error::Error,
};

#[derive(Debug, new)]
pub struct LocalDiskCacheBackend {
    #[new(into)]
    root: PathBuf,
    #[new(default)]
    root_ready: tokio::sync::OnceCell<()>,
}

impl LocalDiskCacheBackend {
    fn path(&self, key: &str, container: &Container) -> Result<PathBuf, Error> {
        let mut parts = std::path::Path::new(key).components();
        let single_normal = matches!(
            (parts.next(), parts.next()),
            (Some(std::path::Component::Normal(_)), None)
        );
        if !single_normal
            || key.contains(['/', '\\', ':'])
            || key == "."
            || key == ".."
            || key.chars().any(|c| c.is_control())
        {
            return Err(Error::custom(eyre::eyre!("invalid key {key:?}")));
        }
        Ok(self.dir(container)?.join(key))
    }

    fn dir(&self, container: &Container) -> Result<PathBuf, Error> {
        self.dir_named(container.as_str())
    }

    fn dir_named(&self, name: &str) -> Result<PathBuf, Error> {
        let path = std::path::Path::new(name);
        if name.contains(':')
            || !path
                .components()
                .all(|part| matches!(part, std::path::Component::Normal(_)))
        {
            return Err(Error::custom(eyre::eyre!("invalid local container")));
        }
        Ok(self.root.join(path))
    }

    /// Whether any component of `path` below the backend root is a symlink.
    /// Stops at the first missing component, which cannot be a symlink.
    async fn crosses_symlink(
        &self,
        path: &std::path::Path,
    ) -> Result<bool, Error> {
        let relative = path.strip_prefix(&self.root).map_err(Error::custom)?;
        let mut current = self.root.clone();
        for part in relative.components() {
            current.push(part);
            match tokio::fs::symlink_metadata(&current).await {
                Ok(meta) if meta.is_symlink() => return Ok(true),
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(false);
                }
                Err(e) => return Err(Error::custom(e)),
            }
        }
        Ok(false)
    }

    /// The directory of a prefix, or `None` when it does not exist. A prefix
    /// that is a symlink or any other non-directory is an error, never followed.
    async fn prefix_root(
        &self,
        prefix: &ContainerPrefix,
    ) -> Result<Option<PathBuf>, Error> {
        let name = prefix.as_str().strip_suffix('/').ok_or_else(|| {
            Error::custom(eyre::eyre!("container prefix must end in '/'"))
        })?;
        let dir = self.dir_named(name)?;
        if self.crosses_symlink(&dir).await? {
            return Err(Error::custom(eyre::eyre!(
                "local prefix {name:?} crosses a symlink"
            )));
        }
        match tokio::fs::symlink_metadata(&dir).await {
            Ok(meta) if meta.is_dir() => Ok(Some(dir)),
            Ok(_) => Err(Error::custom(eyre::eyre!(
                "local prefix {name:?} is not a directory"
            ))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::custom(e)),
        }
    }

    /// Every regular file under `dir` as (key relative to `dir` with '/'
    /// separators, size). Symlinks and other special entries are skipped
    /// without being followed.
    async fn regular_files(
        dir: &std::path::Path,
    ) -> Result<Vec<(String, u64)>, Error> {
        let mut files = Vec::new();
        let mut pending = vec![(dir.to_path_buf(), String::new())];
        while let Some((current, rel)) = pending.pop() {
            let mut entries = match tokio::fs::read_dir(&current).await {
                Ok(entries) => entries,
                // Removed while walking (a concurrent purge): nothing left.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(Error::custom(e)),
            };
            while let Some(entry) =
                entries.next_entry().await.map_err(Error::custom)?
            {
                // A name that is not UTF-8 cannot be a key we wrote, so it is
                // not listed.
                let Ok(name) = entry.file_name().into_string() else {
                    continue;
                };
                let key = if rel.is_empty() {
                    name
                } else {
                    format!("{rel}/{name}")
                };
                let Ok(file_type) = entry.file_type().await else {
                    continue;
                };
                if file_type.is_dir() {
                    pending.push((entry.path(), key));
                } else if file_type.is_file() {
                    let Ok(meta) = entry.metadata().await else {
                        continue;
                    };
                    files.push((key, meta.len()));
                }
            }
        }
        Ok(files)
    }

    /// Number of regular files under `dir`, whatever their names. Tolerates
    /// entries that vanish while walking.
    async fn count_regular_files(dir: &std::path::Path) -> u64 {
        let mut count = 0;
        let mut pending = vec![dir.to_path_buf()];
        while let Some(current) = pending.pop() {
            let Ok(mut entries) = tokio::fs::read_dir(&current).await else {
                continue;
            };
            while let Ok(Some(entry)) = entries.next_entry().await {
                match entry.file_type().await {
                    Ok(t) if t.is_dir() => pending.push(entry.path()),
                    Ok(t) if t.is_file() => count += 1,
                    _ => {}
                }
            }
        }
        count
    }

    /// Whether any regular file exists under `dir`; stops at the first.
    async fn contains_file(dir: std::path::PathBuf) -> bool {
        let mut pending = vec![dir];
        while let Some(current) = pending.pop() {
            let Ok(mut entries) = tokio::fs::read_dir(&current).await else {
                continue;
            };
            while let Ok(Some(entry)) = entries.next_entry().await {
                match entry.file_type().await {
                    Ok(t) if t.is_file() => return true,
                    Ok(t) if t.is_dir() => pending.push(entry.path()),
                    _ => {}
                }
            }
        }
        false
    }

    async fn initialize_root(&self) -> Result<(), Error> {
        self.root_ready
            .get_or_try_init(|| async {
                tokio::fs::create_dir_all(&self.root)
                    .await
                    .map_err(Error::custom)?;
                // An existing directory may belong to a creator cancelled before
                // fsync. Every backend instance establishes its own durable anchor
                // once, including all ancestors above the configured root.
                let root = tokio::fs::canonicalize(&self.root)
                    .await
                    .map_err(Error::custom)?;
                for dir in root.ancestors() {
                    Self::sync_directory(dir).await?;
                }
                Ok::<(), Error>(())
            })
            .await?;
        Ok(())
    }

    async fn create_container_directories(
        &self,
        destination: &std::path::Path,
    ) -> Result<(), Error> {
        let absolute =
            std::path::absolute(destination).map_err(Error::custom)?;
        let mut missing = Vec::new();
        for dir in absolute
            .parent()
            .expect("artifact has a parent")
            .ancestors()
        {
            if tokio::fs::try_exists(dir).await.map_err(Error::custom)? {
                break;
            }
            missing.push(dir.to_path_buf());
        }
        for dir in missing.into_iter().rev() {
            match tokio::fs::create_dir(&dir).await {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(Error::custom(e)),
            }
            // Also sync when a racing creator won: it may not have synced yet.
            Self::sync_directory(
                dir.parent().expect("created directory has a parent"),
            )
            .await?;
        }
        Ok(())
    }

    async fn sync_publication_directories(
        &self,
        destination: &std::path::Path,
    ) -> Result<(), Error> {
        // Sync through the backend root (also the staging parent), never
        // reopening unrelated ancestors on ordinary uploads.
        let root = std::path::absolute(&self.root).map_err(Error::custom)?;
        let absolute =
            std::path::absolute(destination).map_err(Error::custom)?;
        for dir in absolute
            .parent()
            .expect("artifact has a parent")
            .ancestors()
        {
            Self::sync_directory(dir).await?;
            if dir == root {
                break;
            }
        }
        Ok(())
    }

    #[cfg(unix)]
    async fn sync_directory(dir: &std::path::Path) -> Result<(), Error> {
        File::open(dir)
            .await
            .map_err(Error::custom)?
            .sync_all()
            .await
            .map_err(Error::custom)
    }

    #[cfg(not(unix))]
    async fn sync_directory(_dir: &std::path::Path) -> Result<(), Error> {
        // Portable Rust has no directory-fsync equivalent on Windows. The
        // file is synced before atomic rename; crash-durable names are not
        // promised on this platform.
        Ok(())
    }
}

#[async_trait]
impl RemoteCacheStorageBackend for LocalDiskCacheBackend {
    async fn get(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<Option<bytes::Bytes>, Error> {
        let path = self.path(key, container)?;

        if tokio::fs::try_exists(&path).await.map_err(Error::custom)? {
            let bytes = tokio::fs::read(&path).await.map_err(Error::custom)?;

            Ok(Some(Bytes::from_owner(bytes)))
        } else {
            Ok(None)
        }
    }

    async fn exists(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<bool, Error> {
        let path = self.path(key, container)?;
        Ok(tokio::fs::try_exists(&path).await.map_err(Error::custom)?)
    }

    async fn get_stream(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<Option<ReadStream>, Error> {
        let path = self.path(key, container)?;

        if tokio::fs::try_exists(&path).await.map_err(Error::custom)? {
            let stream = ReaderStream::new(
                File::open(path).await.map_err(Error::custom)?,
            )
            .map(|r| r.map_err(Error::custom));

            Ok(Some(Box::pin(stream) as ReadStream))
        } else {
            Ok(None)
        }
    }

    async fn get_range(
        &self,
        container: &Container,
        key: &str,
        range: ByteRange,
    ) -> Result<Option<Bytes>, Error> {
        let path = self.path(key, container)?;

        if self.crosses_symlink(&path).await? {
            return Ok(None);
        }
        let meta = match tokio::fs::symlink_metadata(&path).await {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(e) => return Err(Error::custom(e)),
        };
        if !meta.is_file() {
            return Ok(None);
        }

        let mut file = File::open(&path).await.map_err(Error::custom)?;
        let len = file.metadata().await.map_err(Error::custom)?.len();
        let selected = range.resolve(len);
        if selected.is_empty() {
            return Ok(Some(Bytes::new()));
        }

        let size = usize::try_from(selected.end - selected.start)
            .map_err(Error::custom)?;
        file.seek(std::io::SeekFrom::Start(selected.start))
            .await
            .map_err(Error::custom)?;
        let mut buf = vec![0u8; size];
        file.read_exact(&mut buf).await.map_err(Error::custom)?;

        Ok(Some(Bytes::from_owner(buf)))
    }

    async fn list(
        &self,
        container: &Container,
    ) -> Result<Vec<ListItem>, Error> {
        // One walk, rather than a walk per page of the provided default.
        let Some(dir) = self.prefix_root(&container.prefix()).await? else {
            return Ok(Vec::new());
        };
        let mut files = Self::regular_files(&dir).await?;
        files.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(files
            .into_iter()
            .map(|(key, len)| ListItem {
                key,
                size: ByteSize::b(len),
            })
            .collect())
    }

    async fn list_prefix(
        &self,
        prefix: &ContainerPrefix,
        after: Option<&str>,
        limit: u32,
    ) -> Result<PrefixPage, Error> {
        let Some(dir) = self.prefix_root(prefix).await? else {
            return Ok(PrefixPage::default());
        };
        let mut files = Self::regular_files(&dir).await?;
        files.sort_by(|a, b| a.0.cmp(&b.0));

        let limit = limit.max(1) as usize;
        let remaining: Vec<ListItem> = files
            .into_iter()
            .filter(|(key, _)| after.is_none_or(|after| key.as_str() > after))
            .map(|(key, len)| ListItem {
                key,
                size: ByteSize::b(len),
            })
            .collect();
        let more = remaining.len() > limit;
        let items: Vec<ListItem> = remaining.into_iter().take(limit).collect();
        let next_after = if more {
            items.last().map(|item| item.key.clone())
        } else {
            None
        };

        Ok(PrefixPage { items, next_after })
    }

    async fn list_children(
        &self,
        prefix: &ContainerPrefix,
    ) -> Result<Vec<String>, Error> {
        let Some(dir) = self.prefix_root(prefix).await? else {
            return Ok(Vec::new());
        };
        let mut children = BTreeSet::new();
        let mut entries =
            tokio::fs::read_dir(&dir).await.map_err(Error::custom)?;
        while let Some(entry) =
            entries.next_entry().await.map_err(Error::custom)?
        {
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            // `file_type` does not follow symlinks.
            let is_dir = entry.file_type().await.is_ok_and(|t| t.is_dir());
            if is_dir && Self::contains_file(entry.path()).await {
                children.insert(name);
            }
        }

        Ok(children.into_iter().collect())
    }

    async fn delete_prefix(
        &self,
        prefix: &ContainerPrefix,
    ) -> Result<u64, Error> {
        let Some(dir) = self.prefix_root(prefix).await? else {
            return Ok(0);
        };
        let removed = Self::count_regular_files(&dir).await;
        tokio::fs::remove_dir_all(&dir)
            .await
            .map_err(Error::custom)?;

        Ok(removed)
    }

    async fn save_if_absent(
        &self,
        container: &Container,
        key: &str,
        value: bytes::Bytes,
    ) -> Result<SaveOutcome, Error> {
        self.save_stream_if_absent(
            container,
            key,
            Box::pin(tokio_stream::once(Ok(value))),
        )
        .await
    }

    async fn save_stream_if_absent(
        &self,
        container: &Container,
        key: &str,
        mut value: WriteStream,
    ) -> Result<SaveOutcome, Error> {
        let path = self.path(key, container)?;

        self.initialize_root().await?;
        self.create_container_directories(&path).await?;

        // Stage outside the container so list/get cannot observe a partial
        // upload. TempPath removes abandoned files on error or cancellation.
        // Keeping staging on the backend filesystem permits atomic rename.
        let temp = tempfile::NamedTempFile::new_in(&self.root)
            .map_err(Error::custom)?;
        let (file, staging) = temp.into_parts();
        let mut file = tokio::io::BufWriter::new(File::from_std(file));

        while let Some(chunk) = value.next().await {
            file.write_all(&chunk?).await.map_err(Error::custom)?;
        }
        file.flush().await.map_err(Error::custom)?;
        file.get_mut().sync_all().await.map_err(Error::custom)?;
        drop(file);
        // Create-only publication: link the staged file to its final name
        // without replacing. An existing entry is left untouched and reported.
        // This is atomic on the backend filesystem; the staged name is removed
        // by `persist_noclobber` on success and by drop on any other outcome.
        let target = path.clone();
        let published = tokio::task::spawn_blocking(move || {
            staging.persist_noclobber(&target)
        })
        .await
        .map_err(Error::custom)?;
        match published {
            Ok(()) => {
                self.sync_publication_directories(&path).await?;
                Ok(SaveOutcome::Stored)
            }
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
                // `e.path` still owns the staged file and removes it on drop.
                Ok(SaveOutcome::AlreadyExists)
            }
            Err(e) => Err(Error::custom(e.error)),
        }
    }

    async fn delete(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<(), Error> {
        let path = self.path(key, container)?;

        if !tokio::fs::try_exists(&path).await.map_err(Error::custom)? {
            return Ok(());
        }

        tokio::fs::remove_file(&path).await.map_err(Error::custom)?;

        Ok(())
    }

    async fn size(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<Option<ByteSize>, Error> {
        let path = self.path(key, container)?;

        if !tokio::fs::try_exists(&path).await.map_err(Error::custom)? {
            return Ok(None);
        }

        let metadata =
            tokio::fs::metadata(&path).await.map_err(Error::custom)?;
        Ok(Some(ByteSize::b(metadata.len())))
    }
}

#[cfg(test)]
mod tests {
    use super::LocalDiskCacheBackend;
    use omni_remote_cache_storage_core::decl_remote_cache_storage_backend_tests;
    fn temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[tokio::test]
    async fn cancelled_root_creator_cannot_leave_other_uploads_using_an_unsynced_anchor()
     {
        use bytes::Bytes;
        use omni_remote_cache_storage_core::{
            Container, RemoteCacheStorageBackend,
        };
        use std::{sync::Arc, time::Duration};
        let parent = temp_dir();
        let root = parent.path().join("new-root");
        let backend = Arc::new(LocalDiskCacheBackend::new(&root));
        let (created, waiting) = tokio::sync::oneshot::channel();
        let creator = backend.clone();
        // Pause inside the same initialization cell after mkdir but before
        // sync, modelling a cancelled first creator deterministically.
        let first = tokio::spawn(async move {
            creator
                .root_ready
                .get_or_try_init(|| async {
                    tokio::fs::create_dir_all(&creator.root).await.unwrap();
                    created.send(()).unwrap();
                    std::future::pending::<Result<(), super::Error>>().await
                })
                .await
                .map(|_| ())
        });
        tokio::time::timeout(Duration::from_secs(5), waiting)
            .await
            .unwrap()
            .unwrap();
        assert!(root.is_dir());
        assert!(backend.root_ready.get().is_none());
        let container = Container::parse("ns/ci").unwrap();
        let upload =
            backend.save_if_absent(&container, "d", Bytes::from("complete"));
        tokio::pin!(upload);
        assert!(
            futures::poll!(&mut upload).is_pending(),
            "a visible root is not yet a durable anchor"
        );
        first.abort();
        assert!(first.await.unwrap_err().is_cancelled());
        tokio::time::timeout(Duration::from_secs(5), upload)
            .await
            .unwrap()
            .unwrap();
        assert!(
            backend.root_ready.get().is_some(),
            "second upload must finish root synchronization"
        );
        assert_eq!(
            backend.get(&container, "d").await.unwrap(),
            Some(Bytes::from("complete"))
        );
    }

    decl_remote_cache_storage_backend_tests!(LocalDiskCacheBackend::new(
        temp_dir().path()
    ));

    #[tokio::test]
    async fn rejects_keys_that_escape_the_container() {
        use bytes::Bytes;
        use omni_remote_cache_storage_core::{
            Container, RemoteCacheStorageBackend,
        };
        let dir = temp_dir();
        let b = LocalDiskCacheBackend::new(dir.path());
        let c = Container::parse("ns/ci").unwrap();
        for key in ["../x", "/abs", "a/b", "..", "", "C:escape", "d:stream"] {
            assert!(
                b.save_if_absent(&c, key, Bytes::from("x")).await.is_err(),
                "{key}"
            );
            assert!(b.get(&c, key).await.is_err(), "{key}");
        }
    }
}
