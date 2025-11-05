use std::{
    io::{self, Cursor},
    path::Path,
};

use serde::{Deserialize, Serialize};
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use system_traits::{FsRead, FsReadAsync, FsWrite, FsWriteAsync};
use thiserror::Error;

pub async fn read_async<'a, 'b, TData, TPath, TSys: FsReadAsync + Send + Sync>(
    path: TPath,
    sys: &TSys,
) -> Result<TData, Error>
where
    TData: for<'de> Deserialize<'de>,
    TPath: Into<&'a Path>,
{
    let path: &'a Path = path.into();
    let ext = path.extension().unwrap_or_default();
    let content = sys.fs_read_async(path).await?;

    deserialize(ext, content)
}

pub fn read<'a, 'b, TData, TPath, TSys: FsRead + Send + Sync>(
    path: TPath,
    sys: &TSys,
) -> Result<TData, Error>
where
    TData: for<'de> Deserialize<'de>,
    TPath: Into<&'a Path>,
{
    let path: &'a Path = path.into();
    let ext = path.extension().unwrap_or_default();
    let content = sys.fs_read(path)?;

    deserialize(ext, content)
}

fn deserialize<TConfig>(
    ext: &std::ffi::OsStr,
    content: std::borrow::Cow<'_, [u8]>,
) -> Result<TConfig, Error>
where
    TConfig: for<'de> Deserialize<'de>,
{
    match ext.to_string_lossy().as_ref() {
        "yaml" | "yml" => Ok(serde_norway::from_slice(&content)?),
        "json" => Ok(serde_json::from_slice(&content)?),
        "toml" => Ok(toml::from_slice(&content)?),
        "bin" => Ok(rmp_serde::from_slice(&content)?),
        ext => {
            Err(ErrorInner::UnsupportedFileExtension(ext.to_string()).into())
        }
    }
}

pub fn write<'a, TConfig, TData, TSys: FsWrite + Send + Sync>(
    path: TData,
    data: &TConfig,
    sys: &TSys,
) -> Result<(), Error>
where
    TConfig: Serialize,
    TData: Into<&'a Path>,
{
    let path: &Path = path.into();
    let ext = path.extension().unwrap_or_default();
    let content = serialize(data, ext)?;
    sys.fs_write(path, content)?;

    Ok(())
}

pub async fn write_async<'a, TData, TPath, TSys: FsWriteAsync + Send + Sync>(
    path: TPath,
    data: &TData,
    sys: &TSys,
) -> Result<(), Error>
where
    TData: Serialize,
    TPath: Into<&'a Path>,
{
    let path: &Path = path.into();
    let ext = path.extension().unwrap_or_default();
    let content = serialize(data, ext)?;
    sys.fs_write_async(path, content).await?;

    Ok(())
}

fn serialize<TConfig>(
    config: &TConfig,
    ext: &std::ffi::OsStr,
) -> Result<Vec<u8>, Error>
where
    TConfig: Serialize,
{
    match ext.to_string_lossy().as_ref() {
        "yaml" | "yml" => {
            let mut writer = Cursor::new(Vec::new());
            serde_norway::to_writer(&mut writer, config)?;
            Ok(writer.into_inner())
        }
        "json" => {
            let mut writer = Cursor::new(Vec::new());
            serde_json::to_writer_pretty(&mut writer, config)?;

            Ok(writer.into_inner())
        }
        "toml" => Ok(toml::to_string(config)?.as_bytes().to_vec()),
        "bin" => Ok(rmp_serde::to_vec(config)?),
        ext => {
            Err(ErrorInner::UnsupportedFileExtension(ext.to_string()).into())
        }
    }
}

#[derive(Error, Debug)]
#[error(transparent)]
pub struct Error(ErrorInner);

impl Error {
    #[allow(unused)]
    pub fn kind(&self) -> ErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<ErrorInner>> From<T> for Error {
    fn from(value: T) -> Self {
        let inner = value.into();
        Self(inner)
    }
}

#[derive(Error, Debug, EnumDiscriminants)]
#[strum_discriminants(vis(pub), name(ErrorKind))]
enum ErrorInner {
    #[error("tried to load an unsupported file extension: {0}")]
    UnsupportedFileExtension(String),

    #[error(transparent)]
    Io(#[from] io::Error),

    #[error(transparent)]
    TomlDeserialize(#[from] toml::de::Error),

    #[error(transparent)]
    TomlSerialize(#[from] toml::ser::Error),

    #[error(transparent)]
    YmlSerde(#[from] serde_norway::Error),

    #[error(transparent)]
    BinDeserialize(#[from] rmp_serde::decode::Error),

    #[error(transparent)]
    BinSeserialize(#[from] rmp_serde::encode::Error),

    #[error(transparent)]
    JsonSerde(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use system_traits::{
        EnvSetCurrentDir, FsCreateDirAll as _, impls::InMemorySys,
    };

    #[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
    struct Data {
        pub test: String,
    }

    fn sys() -> InMemorySys {
        let sys = InMemorySys::default();
        let dir = Path::new("test");
        sys.fs_create_dir_all(dir).expect("should create dir");
        sys.env_set_current_dir(dir)
            .expect("should set current dir");

        sys
    }

    fn data() -> Data {
        Data {
            test: "test".to_string(),
        }
    }

    macro_rules! test_write_async {
        ($name:ident, $format:expr) => {
            #[tokio::test]
            async fn $name() {
                let sys = sys();
                let data = data();
                let path = Path::new(concat!("test.", $format));

                write_async(path, &data, &sys).await.expect("can't write");

                let read: Data =
                    read_async(path, &sys).await.expect("can't read");

                assert_eq!(read, data);
            }
        };
    }

    macro_rules! test_write_sync {
        ($name:ident, $format:expr) => {
            #[test]
            fn $name() {
                let sys = sys();
                let data = data();
                let path = Path::new(concat!("test.", $format));

                write(path, &data, &sys).expect("can't write");

                let read: Data = read(path, &sys).expect("can't read");

                assert_eq!(read, data);
            }
        };
    }

    macro_rules! test_read_async {
        ($name:ident, $format:expr) => {
            #[tokio::test]
            async fn $name() {
                let sys = sys();
                let data = data();
                let path = Path::new(concat!("test.", $format));

                write_async(path, &data, &sys).await.expect("can't write");

                let read: Data =
                    read_async(path, &sys).await.expect("can't read");

                assert_eq!(read, data);
            }
        };
    }

    macro_rules! test_read_sync {
        ($name:ident, $format:expr) => {
            #[test]
            fn $name() {
                let sys = sys();
                let data = data();
                let path = Path::new(concat!("test.", $format));

                write(path, &data, &sys).expect("can't write");

                let read: Data = read(path, &sys).expect("can't read");

                assert_eq!(read, data);
            }
        };
    }

    test_write_async!(test_write_yaml_async, "yaml");
    test_write_async!(test_write_json_async, "json");
    test_write_async!(test_write_toml_async, "toml");
    test_write_async!(test_write_bin_async, "bin");

    test_write_sync!(test_write_yaml_sync, "yaml");
    test_write_sync!(test_write_json_sync, "json");
    test_write_sync!(test_write_toml_sync, "toml");
    test_write_sync!(test_write_bin_sync, "bin");

    test_read_async!(test_read_yaml_async, "yaml");
    test_read_async!(test_read_json_async, "json");
    test_read_async!(test_read_toml_async, "toml");
    test_read_async!(test_read_bin_async, "bin");

    test_read_sync!(test_read_yaml_sync, "yaml");
    test_read_sync!(test_read_json_sync, "json");
    test_read_sync!(test_read_toml_sync, "toml");
    test_read_sync!(test_read_bin_sync, "bin");
}
