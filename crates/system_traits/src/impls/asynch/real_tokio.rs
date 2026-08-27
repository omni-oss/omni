use std::{borrow::Cow, io};

use async_trait::async_trait;

use crate::{
    BaseEnvSetCurrentDirAsync, BaseFsAppendAsync, BaseFsCanonicalizeAsync,
    BaseFsCopyAsync, BaseFsCreateDirAsync, BaseFsCreateJunctionAsync,
    BaseFsHardLinkAsync, BaseFsMetadataAsync, BaseFsReadAsync,
    BaseFsReadDirAsync, BaseFsReadLinkAsync, BaseFsRemoveDirAllAsync,
    BaseFsRemoveDirAsync, BaseFsRemoveFileAsync, BaseFsRenameAsync,
    BaseFsSymlinkDirAsync, BaseFsSymlinkFileAsync, BaseFsWriteAsync,
    EnvCurrentDirAsync, auto_impl,
    impls::{RealFsMetadata, RealSys},
};

async fn spawn_blocking<F, R>(f: F) -> R
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .expect("Failed to spawn blocking task")
}

#[async_trait]
impl BaseEnvSetCurrentDirAsync for RealSys {
    async fn base_env_set_current_dir_async(
        &self,
        path: &std::path::Path,
    ) -> io::Result<()> {
        let path = path.to_path_buf();
        spawn_blocking(move || std::env::set_current_dir(path)).await
    }
}

#[async_trait]
impl EnvCurrentDirAsync for RealSys {
    async fn env_current_dir_async(&self) -> io::Result<std::path::PathBuf> {
        spawn_blocking(std::env::current_dir).await
    }
}

#[async_trait]
impl BaseFsWriteAsync for RealSys {
    async fn base_fs_write_async(
        &self,
        path: &std::path::Path,
        data: &[u8],
    ) -> io::Result<()> {
        tokio::fs::write(path, data).await
    }
}

#[async_trait]
impl BaseFsReadAsync for RealSys {
    async fn base_fs_read_async(
        &self,
        path: &std::path::Path,
    ) -> io::Result<Cow<'static, [u8]>> {
        tokio::fs::read(path).await.map(Cow::Owned)
    }
}

#[async_trait]
impl BaseFsCreateDirAsync for RealSys {
    async fn base_fs_create_dir_async(
        &self,
        path: &std::path::Path,
        options: &crate::CreateDirOptions,
    ) -> io::Result<()> {
        let mut builder = &mut tokio::fs::DirBuilder::new();

        if options.recursive {
            builder = builder.recursive(true);
        }

        #[cfg(unix)]
        if let Some(mode) = options.mode {
            builder = builder.mode(mode);
        }

        builder.create(path).await
    }
}

#[async_trait]
impl BaseFsCanonicalizeAsync for RealSys {
    async fn base_fs_canonicalize_async(
        &self,
        path: &std::path::Path,
    ) -> io::Result<std::path::PathBuf> {
        tokio::fs::canonicalize(path).await
    }
}

fn to_real_fs_metadata(value: std::fs::Metadata) -> RealFsMetadata {
    #[allow(unused)]
    struct TokioRealFsMetadata(std::fs::Metadata);

    impl From<TokioRealFsMetadata> for RealFsMetadata {
        #[inline(always)]
        fn from(value: TokioRealFsMetadata) -> Self {
            unsafe {
                std::mem::transmute::<TokioRealFsMetadata, RealFsMetadata>(
                    value,
                )
            }
        }
    }

    TokioRealFsMetadata(value).into()
}

#[async_trait]
impl BaseFsMetadataAsync for RealSys {
    type Metadata = RealFsMetadata;

    async fn base_fs_metadata_async(
        &self,
        path: &std::path::Path,
    ) -> io::Result<Self::Metadata> {
        tokio::fs::metadata(path).await.map(to_real_fs_metadata)
    }

    async fn base_fs_symlink_metadata_async(
        &self,
        path: &std::path::Path,
    ) -> io::Result<Self::Metadata> {
        tokio::fs::symlink_metadata(path)
            .await
            .map(to_real_fs_metadata)
    }
}

#[async_trait]
impl BaseFsRemoveDirAllAsync for RealSys {
    async fn base_fs_remove_dir_all_async(
        &self,
        path: &std::path::Path,
    ) -> io::Result<()> {
        tokio::fs::remove_dir_all(path).await
    }
}

#[async_trait]
impl BaseFsRemoveDirAsync for RealSys {
    async fn base_fs_remove_dir_async(
        &self,
        path: &std::path::Path,
    ) -> io::Result<()> {
        tokio::fs::remove_dir(path).await
    }
}

#[async_trait]
impl BaseFsHardLinkAsync for RealSys {
    async fn base_fs_hard_link_async(
        &self,
        src: &std::path::Path,
        dst: &std::path::Path,
    ) -> io::Result<()> {
        tokio::fs::hard_link(src, dst).await
    }
}

#[async_trait]
impl BaseFsSymlinkFileAsync for RealSys {
    async fn base_fs_symlink_file_async(
        &self,
        original: &std::path::Path,
        link: &std::path::Path,
    ) -> io::Result<()> {
        #[cfg(unix)]
        {
            tokio::fs::symlink(original, link).await
        }
        #[cfg(windows)]
        {
            tokio::fs::symlink_file(original, link).await
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (original, link);
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "symlinks are not supported on this platform",
            ))
        }
    }
}

#[async_trait]
impl BaseFsSymlinkDirAsync for RealSys {
    async fn base_fs_symlink_dir_async(
        &self,
        original: &std::path::Path,
        link: &std::path::Path,
    ) -> io::Result<()> {
        #[cfg(unix)]
        {
            tokio::fs::symlink(original, link).await
        }
        #[cfg(windows)]
        {
            tokio::fs::symlink_dir(original, link).await
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (original, link);
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "symlinks are not supported on this platform",
            ))
        }
    }
}

#[async_trait]
impl BaseFsCreateJunctionAsync for RealSys {
    async fn base_fs_create_junction_async(
        &self,
        original: &std::path::Path,
        junction: &std::path::Path,
    ) -> io::Result<()> {
        use sys_traits::BaseFsCreateJunction;
        let original = original.to_path_buf();
        let junction = junction.to_path_buf();
        spawn_blocking(move || {
            RealSys.base_fs_create_junction(&original, &junction)
        })
        .await
    }
}

#[async_trait]
impl BaseFsReadLinkAsync for RealSys {
    async fn base_fs_read_link_async(
        &self,
        path: &std::path::Path,
    ) -> io::Result<std::path::PathBuf> {
        tokio::fs::read_link(path).await
    }
}

#[async_trait]
impl BaseFsRenameAsync for RealSys {
    async fn base_fs_rename_async(
        &self,
        from: &std::path::Path,
        to: &std::path::Path,
    ) -> io::Result<()> {
        tokio::fs::rename(from, to).await
    }
}

#[async_trait]
impl BaseFsRemoveFileAsync for RealSys {
    async fn base_fs_remove_file_async(
        &self,
        path: &std::path::Path,
    ) -> io::Result<()> {
        tokio::fs::remove_file(path).await
    }
}

#[async_trait]
impl BaseFsCopyAsync for RealSys {
    async fn base_fs_copy_async(
        &self,
        from: &std::path::Path,
        to: &std::path::Path,
    ) -> io::Result<u64> {
        tokio::fs::copy(from, to).await
    }
}

#[async_trait]
impl BaseFsReadDirAsync for RealSys {
    async fn base_fs_read_dir_async(
        &self,
        path: &std::path::Path,
    ) -> io::Result<Vec<std::path::PathBuf>> {
        let mut read_dir = tokio::fs::read_dir(path).await?;
        let mut entries = Vec::new();
        while let Some(entry) = read_dir.next_entry().await? {
            entries.push(entry.path());
        }
        Ok(entries)
    }
}

#[async_trait]
impl BaseFsAppendAsync for RealSys {
    async fn base_fs_append_async(
        &self,
        path: &std::path::Path,
        data: &[u8],
    ) -> io::Result<()> {
        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
            .await?;
        file.write_all(data).await?;
        file.flush().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        FsCreateJunctionAsync, FsMetadataAsync, FsReadLinkAsync,
        FsSymlinkDirAsync, FsSymlinkFileAsync,
    };

    #[tokio::test]
    async fn symlink_file_then_read_link_round_trips() {
        let sys = RealSys;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.txt");
        std::fs::write(&target, b"hello").unwrap();
        let link = dir.path().join("link.txt");

        sys.fs_symlink_file_async(&target, &link).await.unwrap();

        assert!(sys.fs_is_symlink_async(&link).await.unwrap());
        assert_eq!(sys.fs_read_link_async(&link).await.unwrap(), target);
    }

    #[tokio::test]
    async fn symlink_dir_then_read_link_round_trips() {
        let sys = RealSys;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target_dir");
        std::fs::create_dir(&target).unwrap();
        let link = dir.path().join("link_dir");

        sys.fs_symlink_dir_async(&target, &link).await.unwrap();

        assert!(sys.fs_is_symlink_async(&link).await.unwrap());
        assert_eq!(sys.fs_read_link_async(&link).await.unwrap(), target);
    }

    #[tokio::test]
    async fn create_junction_on_dir() {
        let sys = RealSys;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target_dir");
        std::fs::create_dir(&target).unwrap();
        let junction = dir.path().join("junction");

        let result = sys.fs_create_junction_async(&target, &junction).await;

        #[cfg(windows)]
        {
            result.unwrap();
            assert!(junction.exists());
        }
        #[cfg(not(windows))]
        {
            // NTFS junctions are Windows-only; the call must report Unsupported.
            assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Unsupported);
        }
    }
}
