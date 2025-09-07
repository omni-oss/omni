use config_utils::ListConfig;
use merge::Merge;

pub fn default_true() -> bool {
    true
}

pub fn list_config_default<T: Merge>() -> ListConfig<T> {
    ListConfig::append(vec![])
}

pub mod fs {
    use std::{io, path::Path};

    use serde::de::DeserializeOwned;
    use strum::{EnumDiscriminants, IntoDiscriminant as _};
    use system_traits::FsReadAsync;
    use thiserror::Error;

    pub async fn load_config<
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
        let ext = path.extension().unwrap_or_default();
        let content = sys.fs_read_to_string_async(path).await?;

        match ext.to_string_lossy().as_ref() {
            "yaml" | "yml" => Ok(serde_yml::from_str(&content)?),
            "json" => Ok(serde_json::from_str(&content)?),
            "toml" => Ok(toml::from_str(&content)?),
            ext => Err(LoadConfigErrorInner::UnsupportedFileExtension(
                ext.to_string(),
            )
            .into()),
        }
    }

    #[derive(Error, Debug)]
    #[error("{inner}")]
    pub struct LoadConfigError {
        #[source]
        inner: LoadConfigErrorInner,
        kind: LoadConfigErrorKind,
    }

    impl LoadConfigError {
        pub fn kind(&self) -> LoadConfigErrorKind {
            self.kind
        }
    }

    impl<T: Into<LoadConfigErrorInner>> From<T> for LoadConfigError {
        fn from(value: T) -> Self {
            let inner = value.into();
            let kind = inner.discriminant();
            Self { inner, kind }
        }
    }

    #[derive(Error, Debug, EnumDiscriminants)]
    #[strum_discriminants(vis(pub), name(LoadConfigErrorKind))]
    enum LoadConfigErrorInner {
        #[error("unsupported file extension: {0}")]
        UnsupportedFileExtension(String),

        #[error(transparent)]
        Io(#[from] io::Error),

        #[error(transparent)]
        TomlDeserialize(#[from] toml::de::Error),

        #[error(transparent)]
        YmlDeserialize(#[from] serde_yml::Error),

        #[error(transparent)]
        JsonDeserialize(#[from] serde_json::Error),
    }
}
