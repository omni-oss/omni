// BaseEnvSetCurrentDir, BaseEnvSetVar, BaseEnvVar, BaseFsCanonicalize,
// BaseFsChown, BaseFsCloneFile, BaseFsCopy, BaseFsCreateDir,
// BaseFsCreateJunction, BaseFsHardLink, BaseFsMetadata, BaseFsOpen,
// BaseFsRead, BaseFsReadDir, BaseFsReadLink, BaseFsRemoveDir,
// BaseFsRemoveDirAll, BaseFsRemoveFile, BaseFsRename, BaseFsSetFileTimes,
// BaseFsSetPermissions, BaseFsSetSymlinkFileTimes, BaseFsSymlinkChown,
// BaseFsSymlinkDir, BaseFsSymlinkFile, BaseFsWrite, BoxableFsFile,
// CreateDirOptions, EnvCacheDir, EnvCurrentDir, EnvHomeDir, EnvProgramsDir,
// EnvSetCurrentDir, EnvSetUmask, EnvSetVar, EnvTempDir, EnvUmask, EnvVar,
// FileType, FsCanonicalize, FsChown, FsCloneFile, FsCopy, FsCreateDir,
// FsCreateDirAll, FsCreateJunction, FsDirEntry, FsFile, FsFileAsRaw,
// FsFileIsTerminal, FsFileLock, FsFileLockMode, FsFileMetadata, FsFileSetLen,
// FsFileSetPermissions, FsFileSetTimes, FsFileSyncAll, FsFileSyncData,
// FsFileTimes, FsHardLink, FsMetadata, FsMetadataValue, FsOpen, FsRead,
// FsReadDir, FsReadLink, FsRemoveDir, FsRemoveDirAll, FsRemoveFile, FsRename,
// FsSetFileTimes, FsSetPermissions, FsSetSymlinkFileTimes, FsSymlinkChown,
// FsSymlinkDir, FsSymlinkFile, FsWrite, SystemRandom, SystemTimeNow,
// ThreadSleep,
//

use crate::shared::{
    CreateDirOptions, FileType, FsMetadataValue, boxed::BoxedFsMetadataValue,
};
use std::{
    borrow::Cow,
    fs,
    io::{self, Error, ErrorKind},
    path::{self, Path, PathBuf},
};

use async_trait::async_trait;

#[async_trait]
pub trait BaseEnvSetCurrentDirAsync {
    async fn base_env_set_current_dir_async(
        &self,
        path: &Path,
    ) -> io::Result<()>;
}

#[async_trait]
pub trait EnvSetCurrentDirAsync: BaseEnvSetCurrentDirAsync {
    async fn env_set_current_dir_async(
        &self,
        path: impl AsRef<Path> + Send,
    ) -> io::Result<()> {
        self.base_env_set_current_dir_async(path.as_ref()).await
    }
}

impl<T: BaseEnvSetCurrentDirAsync> EnvSetCurrentDirAsync for T {}

#[async_trait]
pub trait BaseFsWriteAsync {
    #[doc(hidden)]
    async fn base_fs_write_async(
        &self,
        path: &Path,
        data: &[u8],
    ) -> io::Result<()>;
}

#[async_trait]
pub trait FsWriteAsync: BaseFsWriteAsync {
    async fn fs_write_async(
        &self,
        path: impl AsRef<Path> + Send,
        data: impl AsRef<[u8]> + Send,
    ) -> io::Result<()> {
        self.base_fs_write_async(path.as_ref(), data.as_ref()).await
    }
}

impl<T: BaseFsWriteAsync> FsWriteAsync for T {}

#[async_trait]
pub trait BaseFsMetadataAsync {
    type Metadata: FsMetadataValue;

    #[doc(hidden)]
    async fn base_fs_metadata_async(
        &self,
        path: &Path,
    ) -> io::Result<Self::Metadata>;

    #[doc(hidden)]
    async fn base_fs_symlink_metadata_async(
        &self,
        path: &Path,
    ) -> io::Result<Self::Metadata>;

    #[doc(hidden)]
    async fn base_fs_exists_async(&self, path: &Path) -> io::Result<bool> {
        match self.base_fs_symlink_metadata_async(path).await {
            Ok(_) => Ok(true),
            Err(err) => {
                if err.kind() == ErrorKind::NotFound {
                    Ok(false)
                } else {
                    Err(err)
                }
            }
        }
    }

    #[doc(hidden)]
    async fn base_fs_exists_no_err_async(&self, path: &Path) -> bool {
        self.base_fs_exists_async(path).await.unwrap_or(false)
    }
}

/// These two functions are so cloesly related that it becomes verbose to
/// separate them out into two traits.
#[async_trait]
pub trait FsMetadataAsync: BaseFsMetadataAsync {
    #[inline]
    async fn fs_metadata_async(
        &self,
        path: impl AsRef<Path> + Send,
    ) -> io::Result<Self::Metadata> {
        self.base_fs_metadata_async(path.as_ref()).await
    }

    #[inline]
    async fn fs_symlink_metadata_async(
        &self,
        path: impl AsRef<Path> + Send,
    ) -> io::Result<Self::Metadata> {
        self.base_fs_symlink_metadata_async(path.as_ref()).await
    }

    #[inline]
    async fn fs_is_file_async(
        &self,
        path: impl AsRef<Path> + Send,
    ) -> io::Result<bool> {
        Ok(self.fs_metadata_async(path).await?.file_type() == FileType::File)
    }

    #[inline]
    async fn fs_is_file_no_err_async(
        &self,
        path: impl AsRef<Path> + Send,
    ) -> bool {
        self.fs_is_file_async(path).await.unwrap_or(false)
    }

    #[inline]
    async fn fs_is_dir_async(
        &self,
        path: impl AsRef<Path> + Send,
    ) -> io::Result<bool> {
        Ok(self.fs_metadata_async(path).await?.file_type() == FileType::Dir)
    }

    #[inline]
    async fn fs_is_dir_no_err_async(
        &self,
        path: impl AsRef<Path> + Send,
    ) -> bool {
        self.fs_is_dir_async(path).await.unwrap_or(false)
    }

    #[inline]
    async fn fs_exists_async(
        &self,
        path: impl AsRef<Path> + Send,
    ) -> io::Result<bool> {
        self.base_fs_exists_async(path.as_ref()).await
    }

    #[inline]
    async fn fs_exists_no_err_async(
        &self,
        path: impl AsRef<Path> + Send,
    ) -> bool {
        self.base_fs_exists_no_err_async(path.as_ref()).await
    }

    #[inline]
    async fn fs_is_symlink_async(
        &self,
        path: impl AsRef<Path> + Send,
    ) -> io::Result<bool> {
        Ok(self.fs_symlink_metadata_async(path).await?.file_type()
            == FileType::Symlink)
    }

    #[inline]
    async fn fs_is_symlink_no_err_async(
        &self,
        path: impl AsRef<Path> + Send,
    ) -> bool {
        self.fs_is_symlink_async(path).await.unwrap_or(false)
    }
}

impl<T: BaseFsMetadataAsync> FsMetadataAsync for T {}

#[async_trait]
pub trait FsFileMetadataAsync {
    async fn fs_file_metadata_async(&self) -> io::Result<BoxedFsMetadataValue>;
}

#[async_trait]
pub trait BaseFsReadAsync {
    async fn base_fs_read_async(
        &self,
        path: &Path,
    ) -> io::Result<Cow<'static, [u8]>>;
}

#[async_trait]
pub trait FsReadAsync: BaseFsReadAsync {
    #[inline]
    async fn fs_read_async(
        &self,
        path: impl AsRef<Path> + Send,
    ) -> io::Result<Cow<'static, [u8]>> {
        self.base_fs_read_async(path.as_ref()).await
    }

    async fn fs_read_to_string_async(
        &self,
        path: impl AsRef<Path> + Send,
    ) -> io::Result<Cow<'static, str>> {
        let bytes = self.fs_read_async(path).await?;
        match bytes {
            Cow::Borrowed(bytes) => str::from_utf8(bytes)
                .map(Cow::Borrowed)
                .map_err(|e| e.to_string()),
            Cow::Owned(bytes) => String::from_utf8(bytes)
                .map(Cow::Owned)
                .map_err(|e| e.to_string()),
        }
        .map_err(|error_text| Error::new(ErrorKind::InvalidData, error_text))
    }

    async fn fs_read_to_string_lossy_async(
        &self,
        path: impl AsRef<Path> + Send,
    ) -> io::Result<Cow<'static, str>> {
        // Like String::from_utf8_lossy but operates on owned values
        #[inline(always)]
        fn string_from_utf8_lossy(buf: Vec<u8>) -> String {
            match String::from_utf8_lossy(&buf) {
                // buf contained non-utf8 chars than have been patched
                Cow::Owned(s) => s,
                // SAFETY: if Borrowed then the buf only contains utf8 chars,
                // we do this instead of .into_owned() to avoid copying the input buf
                Cow::Borrowed(_) => unsafe { String::from_utf8_unchecked(buf) },
            }
        }

        let bytes = self.fs_read_async(path).await?;
        match bytes {
            Cow::Borrowed(bytes) => Ok(String::from_utf8_lossy(bytes)),
            Cow::Owned(bytes) => Ok(Cow::Owned(string_from_utf8_lossy(bytes))),
        }
    }
}

impl<T: BaseFsReadAsync> FsReadAsync for T {}

#[async_trait]
pub trait EnvCurrentDirAsync {
    async fn env_current_dir_async(&self) -> io::Result<PathBuf>;
}

#[async_trait]
pub trait BaseFsCreateDirAsync {
    #[doc(hidden)]
    async fn base_fs_create_dir_async(
        &self,
        path: &Path,
        options: &CreateDirOptions,
    ) -> io::Result<()>;
}

#[async_trait]
pub trait FsCreateDirAsync: BaseFsCreateDirAsync {
    async fn fs_create_dir_async(
        &self,
        path: impl AsRef<Path> + Send,
        options: &CreateDirOptions,
    ) -> io::Result<()> {
        self.base_fs_create_dir_async(path.as_ref(), options).await
    }
}

impl<T: BaseFsCreateDirAsync> FsCreateDirAsync for T {}

// == FsCreateDirAll ==

#[async_trait]
pub trait FsCreateDirAllAsync: BaseFsCreateDirAsync {
    async fn fs_create_dir_all_async(
        &self,
        path: impl AsRef<Path> + Send,
    ) -> io::Result<()> {
        self.base_fs_create_dir_async(
            path.as_ref(),
            &CreateDirOptions::new_recursive(),
        )
        .await
    }
}

impl<T: BaseFsCreateDirAsync> FsCreateDirAllAsync for T {}

#[async_trait]
pub trait BaseFsCanonicalizeAsync {
    async fn base_fs_canonicalize_async(
        &self,
        path: &Path,
    ) -> io::Result<PathBuf>;
}

#[async_trait]
pub trait FsCanonicalizeAsync: BaseFsCanonicalizeAsync {
    #[inline]
    async fn fs_canonicalize_async(
        &self,
        path: impl AsRef<Path> + Send,
    ) -> io::Result<PathBuf> {
        self.base_fs_canonicalize_async(path.as_ref()).await
    }
}

impl<T: BaseFsCanonicalizeAsync> FsCanonicalizeAsync for T {}
