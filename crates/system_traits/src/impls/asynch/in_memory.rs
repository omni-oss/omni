use sys_traits::{
    FsHardLink as _, FsMetadata as _, boxed::BoxedFsMetadataValue,
    impls::InMemorySys,
};

use crate::{
    BaseFsCanonicalizeAsync, BaseFsHardLinkAsync, BaseFsMetadataAsync,
};

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
    type Metadata = BoxedFsMetadataValue;

    async fn base_fs_metadata_async(
        &self,
        path: &std::path::Path,
    ) -> std::io::Result<Self::Metadata> {
        Ok(BoxedFsMetadataValue::new(self.fs_metadata(path)?))
    }

    async fn base_fs_symlink_metadata_async(
        &self,
        path: &std::path::Path,
    ) -> std::io::Result<Self::Metadata> {
        Ok(BoxedFsMetadataValue::new(self.fs_symlink_metadata(path)?))
    }
}
