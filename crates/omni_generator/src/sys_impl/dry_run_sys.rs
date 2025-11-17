use std::{borrow::Cow, io, path::Path};

use derive_new::new;
use system_traits::{
    BaseFsCreateDir, BaseFsCreateDirAsync, BaseFsMetadataAsync,
    BaseFsReadAsync, BaseFsRemoveDir as _, BaseFsRemoveDirAll as _,
    BaseFsRemoveDirAllAsync, BaseFsRemoveDirAsync, BaseFsRemoveFileAsync,
    BaseFsWriteAsync, CreateDirOptions, FileType, FsCreateDirAll,
    FsMetadataAsync as _, FsMetadataValue, FsRemoveFile,
    boxed::BoxedFsMetadataValue,
    impls::{InMemorySys, RealSys},
};

#[derive(Clone, Default, new)]
pub struct DryRunSys {
    in_memory: InMemorySys,
    real: RealSys,
}

#[async_trait::async_trait]
impl BaseFsReadAsync for DryRunSys {
    async fn base_fs_read_async(
        &self,
        path: &Path,
    ) -> io::Result<Cow<'static, [u8]>> {
        if let Ok(data) = self.in_memory.base_fs_read_async(path).await {
            return Ok(data);
        }

        let content = self.real.base_fs_read_async(path).await?;

        let dir = path.parent().expect("should have directory");
        if !self.in_memory.fs_exists_async(dir).await? {
            self.in_memory.fs_create_dir_all(path)?;
        }

        Ok(content)
    }
}

#[async_trait::async_trait]
impl BaseFsWriteAsync for DryRunSys {
    async fn base_fs_write_async(
        &self,
        path: &Path,
        data: &[u8],
    ) -> io::Result<()> {
        trace::info!("Dry run: writing to path: {}", path.display());

        self.in_memory.base_fs_write_async(path, data).await
    }
}

#[async_trait::async_trait]
impl BaseFsMetadataAsync for DryRunSys {
    type Metadata = BoxedFsMetadataValue;

    #[doc(hidden)]
    async fn base_fs_metadata_async(
        &self,
        path: &Path,
    ) -> io::Result<Self::Metadata> {
        if self.in_memory.fs_exists_async(path).await?
            && let Ok(metadata) =
                self.in_memory.base_fs_metadata_async(path).await
        {
            return Ok(metadata);
        }

        let result = self
            .real
            .base_fs_metadata_async(path)
            .await
            .map(BoxedFsMetadataValue::new)?;

        if result.file_type() == FileType::Dir {
            self.in_memory.fs_create_dir_all(path)?;
        }

        Ok(result)
    }

    #[doc(hidden)]
    async fn base_fs_symlink_metadata_async(
        &self,
        path: &Path,
    ) -> io::Result<Self::Metadata> {
        if self.in_memory.fs_exists_async(path).await?
            && let Ok(metadata) =
                self.in_memory.base_fs_symlink_metadata_async(path).await
        {
            return Ok(metadata);
        }

        self.real
            .base_fs_symlink_metadata_async(path)
            .await
            .map(BoxedFsMetadataValue::new)
    }
}

#[async_trait::async_trait]
impl BaseFsCreateDirAsync for DryRunSys {
    async fn base_fs_create_dir_async(
        &self,
        path: &Path,
        options: &CreateDirOptions,
    ) -> io::Result<()> {
        trace::info!("Dry run: creating directory: {}", path.display());
        self.in_memory.base_fs_create_dir(path, options)
    }
}

#[async_trait::async_trait]
impl BaseFsRemoveDirAsync for DryRunSys {
    async fn base_fs_remove_dir_async(&self, path: &Path) -> io::Result<()> {
        trace::info!("Dry run: removing directory: {}", path.display());
        self.in_memory.base_fs_remove_dir(path)
    }
}

#[async_trait::async_trait]
impl BaseFsRemoveDirAllAsync for DryRunSys {
    async fn base_fs_remove_dir_all_async(
        &self,
        path: &Path,
    ) -> io::Result<()> {
        trace::info!(
            "Dry run: removing directory and all of its contents: {}",
            path.display()
        );
        self.in_memory.base_fs_remove_dir_all(path)
    }
}

#[async_trait::async_trait]
impl BaseFsRemoveFileAsync for DryRunSys {
    async fn base_fs_remove_file_async(&self, path: &Path) -> io::Result<()> {
        trace::info!("Dry run: removing file: {}", path.display());
        self.in_memory.fs_remove_file(path)
    }
}
