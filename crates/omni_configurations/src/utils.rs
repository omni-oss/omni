use config_utils::ListConfig;
use merge::Merge;
use omni_config_types::MergeSingleOrMany;
use omni_glob_config::MergeGlobConfig;

#[inline(always)]
pub fn default_true() -> bool {
    true
}

#[inline(always)]
pub fn list_config_default<T: Merge>() -> ListConfig<T> {
    ListConfig::append(vec![])
}

/// A glob field left unset appends nothing onto the layer below it, so a
/// project extends a workspace default instead of replacing it.
#[inline(always)]
pub fn merge_glob_config_default<T: Merge>() -> MergeGlobConfig<T> {
    MergeGlobConfig::Many(MergeSingleOrMany::List(ListConfig::append(vec![])))
}

pub mod fs {
    use std::{
        io,
        path::{Path, PathBuf},
    };

    use derive_new::new;
    use omni_file_data_serde::ext_to_format;
    use serde::de::DeserializeOwned;
    use strum::{
        Display, EnumDiscriminants, EnumString, IntoDiscriminant as _,
    };
    use system_traits::{FsRead, FsReadAsync};
    use thiserror::Error;

    #[allow(clippy::result_large_err)]
    pub async fn load_config_async<
        'a,
        'b,
        TConfig,
        TPath,
        TSys: FsReadAsync + Send + Sync,
    >(
        path: TPath,
        sys: &TSys,
    ) -> Result<TConfig, LoadConfigError>
    where
        TConfig: DeserializeOwned,
        TPath: Into<&'a Path>,
    {
        let path: &'a Path = path.into();
        let ext = ext_to_format(path.extension().unwrap_or_default())
            .map_err(|e| (path.to_path_buf(), e))?;
        let content = sys.fs_read_async(path).await.map_err(|e| {
            (
                PathBuf::from(path),
                if e.kind() == std::io::ErrorKind::NotFound {
                    LoadConfigErrorInner::new_file_not_found(path.to_path_buf())
                } else {
                    LoadConfigErrorInner::Io(e)
                },
            )
        })?;

        Ok(omni_file_data_serde::from_slice(&content, ext)
            .map_err(|e| (path.to_path_buf(), e))?)
    }

    #[allow(clippy::result_large_err)]
    pub fn load_config<'a, 'b, TConfig, TPath, TSys: FsRead + Send + Sync>(
        path: TPath,
        sys: &TSys,
    ) -> Result<TConfig, LoadConfigError>
    where
        TConfig: DeserializeOwned,
        TPath: Into<&'a Path>,
    {
        let path: &'a Path = path.into();
        let ext = ext_to_format(path.extension().unwrap_or_default())
            .map_err(|e| (path.to_path_buf(), e))?;
        let content = sys.fs_read(path).map_err(|e| {
            (
                PathBuf::from(path),
                if e.kind() == std::io::ErrorKind::NotFound {
                    LoadConfigErrorInner::new_file_not_found(path.to_path_buf())
                } else {
                    LoadConfigErrorInner::Io(e)
                },
            )
        })?;

        Ok(omni_file_data_serde::from_slice(&content, ext)
            .map_err(|e| (path.to_path_buf(), e))?)
    }

    #[derive(Error, Debug, new)]
    #[error("({kind}) error when loading config from {path}", kind = self.kind())]
    pub struct LoadConfigError {
        #[source]
        error: LoadConfigErrorInner,

        path: PathBuf,
    }

    impl LoadConfigError {
        pub fn kind(&self) -> LoadConfigErrorKind {
            self.error.discriminant()
        }
    }

    impl<T: Into<LoadConfigErrorInner>> From<(PathBuf, T)> for LoadConfigError {
        fn from(value: (PathBuf, T)) -> Self {
            let inner = value.1.into();
            Self {
                path: value.0,
                error: inner,
            }
        }
    }

    #[derive(Error, Debug, EnumDiscriminants, new)]
    #[strum_discriminants(
        vis(pub),
        name(LoadConfigErrorKind),
        derive(EnumString, Display)
    )]
    enum LoadConfigErrorInner {
        #[error(transparent)]
        Io(#[from] io::Error),

        #[error("file not found: {path}", path = path.display())]
        FileNotFound { path: PathBuf },

        #[error(transparent)]
        FileDataSerde(#[from] omni_file_data_serde::Error),
    }
}
