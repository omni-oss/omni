use sys_traits::{
    BaseFsCreateDir as _, BaseFsRemoveDir as _, BaseFsRemoveDirAll as _,
    EnvCurrentDir as _, EnvSetCurrentDir as _, FsCopy as _, FsDirEntry as _,
    FsHardLink as _, FsMetadata as _, FsRead as _, FsReadDir as _,
    FsRemoveFile as _, FsRename as _, FsWrite as _,
    impls::{InMemoryMetadata, InMemorySys},
};

use crate::{
    BaseEnvSetCurrentDirAsync, BaseFsAppendAsync, BaseFsCanonicalizeAsync,
    BaseFsCopyAsync, BaseFsCreateDirAsync, BaseFsHardLinkAsync,
    BaseFsMetadataAsync, BaseFsReadAsync, BaseFsReadDirAsync,
    BaseFsRemoveDirAllAsync, BaseFsRemoveDirAsync, BaseFsRemoveFileAsync,
    BaseFsRenameAsync, BaseFsWriteAsync, CreateDirOptions, EnvCurrentDirAsync,
};

// ---------------------------------------------------------------------------
// Existing impls (kept as-is)
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
impl BaseFsCanonicalizeAsync for InMemorySys {
    async fn base_fs_canonicalize_async(
        &self,
        path: &std::path::Path,
    ) -> std::io::Result<std::path::PathBuf> {
        Ok(path.canonicalize()?)
    }
}

#[async_trait::async_trait]
impl BaseFsHardLinkAsync for InMemorySys {
    async fn base_fs_hard_link_async(
        &self,
        src: &std::path::Path,
        dst: &std::path::Path,
    ) -> std::io::Result<()> {
        Ok(self.fs_hard_link(src, dst)?)
    }
}

#[async_trait::async_trait]
impl BaseFsMetadataAsync for InMemorySys {
    type Metadata = InMemoryMetadata;

    async fn base_fs_metadata_async(
        &self,
        path: &std::path::Path,
    ) -> std::io::Result<Self::Metadata> {
        self.fs_metadata(path)
    }

    async fn base_fs_symlink_metadata_async(
        &self,
        path: &std::path::Path,
    ) -> std::io::Result<Self::Metadata> {
        self.fs_symlink_metadata(path)
    }
}

#[async_trait::async_trait]
impl BaseFsReadAsync for InMemorySys {
    async fn base_fs_read_async(
        &self,
        path: &std::path::Path,
    ) -> std::io::Result<std::borrow::Cow<'static, [u8]>> {
        Ok(self.fs_read(path)?)
    }
}

#[async_trait::async_trait]
impl BaseFsWriteAsync for InMemorySys {
    async fn base_fs_write_async(
        &self,
        path: &std::path::Path,
        data: &[u8],
    ) -> std::io::Result<()> {
        Ok(self.fs_write(path, data)?)
    }
}

#[async_trait::async_trait]
impl BaseFsCopyAsync for InMemorySys {
    async fn base_fs_copy_async(
        &self,
        from: &std::path::Path,
        to: &std::path::Path,
    ) -> std::io::Result<u64> {
        Ok(self.fs_copy(from, to)?)
    }
}

#[async_trait::async_trait]
impl BaseFsReadDirAsync for InMemorySys {
    async fn base_fs_read_dir_async(
        &self,
        path: &std::path::Path,
    ) -> std::io::Result<Vec<std::path::PathBuf>> {
        let iter = self.fs_read_dir(path)?;
        let mut entries = Vec::new();
        for entry in iter {
            let entry = entry?;
            entries.push(entry.path().into_owned());
        }
        Ok(entries)
    }
}

#[async_trait::async_trait]
impl BaseFsAppendAsync for InMemorySys {
    async fn base_fs_append_async(
        &self,
        path: &std::path::Path,
        data: &[u8],
    ) -> std::io::Result<()> {
        // InMemorySys does not directly support append-mode file handles via
        // the async API surface, so emulate it with a read-modify-write.
        let existing = match self.fs_read(path) {
            Ok(bytes) => bytes.into_owned(),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                Vec::new()
            }
            Err(err) => return Err(err),
        };

        let mut combined = existing;
        combined.extend_from_slice(data);
        Ok(self.fs_write(path, &combined)?)
    }
}

// ---------------------------------------------------------------------------
// New impls – required for InMemorySys to satisfy FsSys + ProcSys
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
impl BaseFsCreateDirAsync for InMemorySys {
    async fn base_fs_create_dir_async(
        &self,
        path: &std::path::Path,
        options: &CreateDirOptions,
    ) -> std::io::Result<()> {
        Ok(self.base_fs_create_dir(path, options)?)
    }
}

#[async_trait::async_trait]
impl BaseFsRemoveDirAsync for InMemorySys {
    async fn base_fs_remove_dir_async(
        &self,
        path: &std::path::Path,
    ) -> std::io::Result<()> {
        Ok(self.base_fs_remove_dir(path)?)
    }
}

#[async_trait::async_trait]
impl BaseFsRemoveDirAllAsync for InMemorySys {
    async fn base_fs_remove_dir_all_async(
        &self,
        path: &std::path::Path,
    ) -> std::io::Result<()> {
        Ok(self.base_fs_remove_dir_all(path)?)
    }
}

#[async_trait::async_trait]
impl BaseFsRemoveFileAsync for InMemorySys {
    async fn base_fs_remove_file_async(
        &self,
        path: &std::path::Path,
    ) -> std::io::Result<()> {
        Ok(self.fs_remove_file(path)?)
    }
}

#[async_trait::async_trait]
impl BaseFsRenameAsync for InMemorySys {
    async fn base_fs_rename_async(
        &self,
        from: &std::path::Path,
        to: &std::path::Path,
    ) -> std::io::Result<()> {
        Ok(self.fs_rename(from, to)?)
    }
}

/// Returns the in-memory working directory tracked by [`InMemorySys`].
#[async_trait::async_trait]
impl EnvCurrentDirAsync for InMemorySys {
    async fn env_current_dir_async(
        &self,
    ) -> std::io::Result<std::path::PathBuf> {
        self.env_current_dir()
    }
}

/// Changes the in-memory working directory tracked by [`InMemorySys`].
#[async_trait::async_trait]
impl BaseEnvSetCurrentDirAsync for InMemorySys {
    async fn base_env_set_current_dir_async(
        &self,
        path: &std::path::Path,
    ) -> std::io::Result<()> {
        Ok(self.env_set_current_dir(path)?)
    }
}
