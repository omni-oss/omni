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

    use omni_file_data_serde::ext_to_format;
    use serde::de::DeserializeOwned;
    use strum::{
        Display, EnumDiscriminants, EnumString, IntoDiscriminant as _,
    };
    use system_traits::{FsRead, FsReadAsync};
    use thiserror::Error;

    omni_codes::prefixes!("config");

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
            .map_err(|e| file_data_failed(path, e))?;
        let content = sys
            .fs_read_async(path)
            .await
            .map_err(|e| read_failed(path, e))?;

        omni_file_data_serde::from_slice(&content, ext)
            .map_err(|e| file_data_failed(path, e))
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
            .map_err(|e| file_data_failed(path, e))?;
        let content = sys.fs_read(path).map_err(|e| read_failed(path, e))?;

        omni_file_data_serde::from_slice(&content, ext)
            .map_err(|e| file_data_failed(path, e))
    }

    pub(crate) fn read_failed(
        path: &Path,
        error: io::Error,
    ) -> LoadConfigError {
        if error.kind() == io::ErrorKind::NotFound {
            LoadConfigErrorInner::FileNotFound {
                path: path.to_path_buf(),
            }
        } else {
            LoadConfigErrorInner::Io {
                path: path.to_path_buf(),
                error,
            }
        }
        .into()
    }

    pub(crate) fn file_data_failed(
        path: &Path,
        error: omni_file_data_serde::Error,
    ) -> LoadConfigError {
        LoadConfigErrorInner::file_data(path, error).into()
    }

    #[derive(Error, Debug, omni_diagnostic::Diagnostic)]
    #[error(
        "({kind}) error when loading config from {path}",
        kind = self.kind(),
        path = self.path().display()
    )]
    #[diagnostic(delegate)]
    pub struct LoadConfigError {
        #[source]
        error: LoadConfigErrorInner,
    }

    impl LoadConfigError {
        pub fn kind(&self) -> LoadConfigErrorKind {
            self.error.discriminant()
        }

        pub fn path(&self) -> &Path {
            self.error.path()
        }
    }

    impl From<LoadConfigErrorInner> for LoadConfigError {
        fn from(error: LoadConfigErrorInner) -> Self {
            Self { error }
        }
    }

    #[derive(Error, Debug, EnumDiscriminants, omni_diagnostic::Diagnostic)]
    #[strum_discriminants(
        vis(pub),
        name(LoadConfigErrorKind),
        derive(EnumString, Display)
    )]
    enum LoadConfigErrorInner {
        /// The configuration file could not be read.
        #[error("{error}")]
        #[diagnostic(code = "config.unreadable", audience = environment, hint = "check permissions and filesystem access for {path:?}")]
        Io {
            #[diagnostic(detail)]
            path: PathBuf,
            #[source]
            error: io::Error,
        },

        /// The config file does not exist.
        #[error("file not found: {path}", path = path.display())]
        #[diagnostic(
            code = "config.file_not_found",
            hint = "create the config file at {path:?}"
        )]
        FileNotFound {
            #[diagnostic(detail)]
            path: PathBuf,
        },

        /// The config file exists but could not be loaded as configuration.
        #[error("{error}")]
        #[diagnostic(
            code = "config.load_failed",
            hint = "fix the config file at {path:?}"
        )]
        FileDataSerde {
            #[diagnostic(detail)]
            path: PathBuf,
            #[diagnostic(detail)]
            kind: String,
            error: omni_file_data_serde::Error,
        },
    }

    impl LoadConfigErrorInner {
        fn file_data(path: &Path, error: omni_file_data_serde::Error) -> Self {
            Self::FileDataSerde {
                path: path.to_path_buf(),
                kind: format!("{:?}", error.kind()),
                error,
            }
        }

        fn path(&self) -> &Path {
            match self {
                Self::Io { path, .. }
                | Self::FileNotFound { path }
                | Self::FileDataSerde { path, .. } => path.as_path(),
            }
        }
    }
}
