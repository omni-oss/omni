#![allow(clippy::redundant_field_names)]

use std::path::PathBuf;

use async_trait::async_trait;
use bytes::Bytes;
use bytesize::ByteSize;
use derive_new::new;
use tokio::{fs::File, io::AsyncWriteExt as _, task::JoinSet};
use tokio_stream::StreamExt;
use tokio_util::io::ReaderStream;

use omni_remote_cache_storage_core::{
    Container, ListItem, PageOptions, ReadStream, RemoteCacheStorageBackend,
    SaveOutcome, WriteStream, error::Error,
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
        let path = std::path::Path::new(container.as_str());
        if container.as_str().contains(':')
            || !path
                .components()
                .all(|part| matches!(part, std::path::Component::Normal(_)))
        {
            return Err(Error::custom(eyre::eyre!("invalid local container")));
        }
        Ok(self.root.join(path))
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

    async fn list(
        &self,
        container: &Container,
    ) -> Result<Vec<ListItem>, Error> {
        let dir = self.dir(container)?;

        if !tokio::fs::try_exists(&dir).await.map_err(Error::custom)? {
            return Ok(Vec::new());
        }

        let mut read_dir =
            tokio::fs::read_dir(&dir).await.map_err(Error::custom)?;

        let mut futs = JoinSet::new();

        while let Some(entry) =
            read_dir.next_entry().await.map_err(Error::custom)?
        {
            let path = entry.path();
            let key = path
                .strip_prefix(&dir)
                .map_err(Error::custom)?
                .to_str()
                .ok_or_else(|| Error::custom(eyre::eyre!("invalid path")))?
                .to_string();
            futs.spawn(async move {
                let metadata =
                    tokio::fs::metadata(&path).await.map_err(Error::custom)?;
                Ok::<_, Error>(ListItem {
                    key,
                    size: ByteSize::b(metadata.len()),
                })
            });
        }

        let results = futs.join_all().await;
        let mut items = Vec::with_capacity(results.len());

        for result in results {
            items.push(result.map_err(Error::custom)?);
        }

        Ok(items)
    }

    async fn paged_list(
        &self,
        container: &Container,
        query: PageOptions,
    ) -> Result<Vec<ListItem>, Error> {
        let all = self.list(container).await?;

        let position = all
            .iter()
            .position(|item| {
                if let Some(after_key) = query.after_key.as_ref() {
                    item.key == *after_key
                } else {
                    true
                }
            })
            .unwrap_or(0);

        Ok(all
            .into_iter()
            .skip(position)
            .take(query.per_page.unwrap_or(100) as usize)
            .collect::<Vec<_>>())
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
