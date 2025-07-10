use std::{borrow::Cow, io};

use async_trait::async_trait;

use crate::{
    BaseEnvSetCurrentDirAsync, BaseFsCanonicalizeAsync, BaseFsCreateDirAsync,
    BaseFsMetadataAsync, BaseFsReadAsync, BaseFsWriteAsync, EnvCurrentDirAsync,
    auto_impl, impls::RealFsMetadata,
};

#[derive(Clone, Debug)]
pub struct TokioRealSysAsync;

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
impl BaseEnvSetCurrentDirAsync for TokioRealSysAsync {
    async fn base_env_set_current_dir_async(
        &self,
        path: &std::path::Path,
    ) -> io::Result<()> {
        let path = path.to_path_buf();
        spawn_blocking(move || std::env::set_current_dir(path)).await
    }
}

#[async_trait]
impl EnvCurrentDirAsync for TokioRealSysAsync {
    async fn env_current_dir_async(&self) -> io::Result<std::path::PathBuf> {
        spawn_blocking(std::env::current_dir).await
    }
}

#[async_trait]
impl BaseFsWriteAsync for TokioRealSysAsync {
    async fn base_fs_write_async(
        &self,
        path: &std::path::Path,
        data: &[u8],
    ) -> io::Result<()> {
        tokio::fs::write(path, data).await
    }
}

#[async_trait]
impl BaseFsReadAsync for TokioRealSysAsync {
    async fn base_fs_read_async(
        &self,
        path: &std::path::Path,
    ) -> io::Result<Cow<'static, [u8]>> {
        tokio::fs::read(path).await.map(Cow::Owned)
    }
}

#[async_trait]
impl BaseFsCreateDirAsync for TokioRealSysAsync {
    async fn base_fs_create_dir_async(
        &self,
        path: &std::path::Path,
        options: &crate::CreateDirOptions,
    ) -> io::Result<()> {
        let mut builder = &mut tokio::fs::DirBuilder::new();

        if options.recursive {
            builder = builder.recursive(true);
        }

        if let Some(mode) = options.mode {
            builder = builder.mode(mode);
        }

        builder.create(path).await
    }
}

#[async_trait]
impl BaseFsCanonicalizeAsync for TokioRealSysAsync {
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
impl BaseFsMetadataAsync for TokioRealSysAsync {
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
