use std::{borrow::Cow, io};

use async_trait::async_trait;

use crate::{
    BaseEnvSetCurrentDirAsync, BaseFsAppendAsync, BaseFsCanonicalizeAsync,
    BaseFsCopyAsync, BaseFsCreateDirAsync, BaseFsHardLinkAsync,
    BaseFsMetadataAsync, BaseFsReadAsync, BaseFsReadDirAsync,
    BaseFsRemoveDirAllAsync, BaseFsRemoveDirAsync, BaseFsRemoveFileAsync,
    BaseFsRenameAsync, BaseFsWriteAsync, EnvCurrentDirAsync, auto_impl,
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
