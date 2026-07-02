use std::{
    io::{self, Cursor},
    path::Path,
};

use eyre::Context;
use serde::{Deserialize, Serialize};
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use system_traits::{FsRead, FsReadAsync, FsWrite, FsWriteAsync};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    Yaml,
    Json,
    Toml,
    Bin,
}

pub async fn read_async<'a, 'b, TData, TPath, TSys: FsReadAsync + Send + Sync>(
    path: TPath,
    sys: &TSys,
) -> Result<TData, Error>
where
    TData: for<'de> Deserialize<'de>,
    TPath: AsRef<Path>,
{
    read_with_format_async(
        ext_to_format(path.as_ref().extension().unwrap_or_default())?,
        path,
        sys,
    )
    .await
}

pub async fn read_with_format_async<
    'a,
    'b,
    TData,
    TPath,
    TSys: FsReadAsync + Send + Sync,
>(
    format: Format,
    path: TPath,
    sys: &TSys,
) -> Result<TData, Error>
where
    TData: for<'de> Deserialize<'de>,
    TPath: AsRef<Path>,
{
    let path = path.as_ref();
    let content = sys.fs_read_async(path).await?;

    from_slice(&content, format)
}

pub fn read<'a, 'b, TData, TPath, TSys: FsRead + Send + Sync>(
    path: TPath,
    sys: &TSys,
) -> Result<TData, Error>
where
    TData: for<'de> Deserialize<'de>,
    TPath: AsRef<Path>,
{
    read_with_format(
        ext_to_format(path.as_ref().extension().unwrap_or_default())?,
        path,
        sys,
    )
}

pub fn read_with_format<'a, 'b, TData, TPath, TSys: FsRead + Send + Sync>(
    format: Format,
    path: TPath,
    sys: &TSys,
) -> Result<TData, Error>
where
    TData: for<'de> Deserialize<'de>,
    TPath: AsRef<Path>,
{
    let path = path.as_ref();
    let content = sys.fs_read(path)?;

    from_slice(&content, format)
}

pub fn from_slice<TData>(data: &[u8], format: Format) -> Result<TData, Error>
where
    TData: for<'de> Deserialize<'de>,
{
    // fast path for TOML, since toml doesn't support from_reader for &[u8]
    if format == Format::Toml {
        return read_toml_from_bytes(data);
    }

    let mut reader = Cursor::new(data);
    from_reader(&mut reader, format)
}

pub fn from_reader<TData>(
    reader: &mut impl std::io::Read,
    format: Format,
) -> Result<TData, Error>
where
    TData: for<'de> Deserialize<'de>,
{
    match format {
        Format::Yaml => {
            let value = noyalib::from_reader(reader)?;
            let deserializer = noyalib::Deserializer::new(&value);
            Ok(serde_path_to_error::deserialize(deserializer)?)
        }
        Format::Json => {
            let mut deserializer =
                serde_json::Deserializer::from_reader(reader);
            Ok(serde_path_to_error::deserialize(&mut deserializer)?)
        }
        Format::Toml => {
            let mut data = Vec::new();
            reader.read_to_end(&mut data)?;
            read_toml_from_bytes(&data)
        }
        Format::Bin => {
            let mut deserializer = rmp_serde::Deserializer::new(reader);
            Ok(serde_path_to_error::deserialize(&mut deserializer)?)
        }
    }
}

fn read_toml_from_bytes<TData>(data: &[u8]) -> Result<TData, Error>
where
    TData: for<'de> Deserialize<'de>,
{
    let utf8_str = std::str::from_utf8(&data)?;
    let deserializer =
        toml::Deserializer::parse(utf8_str).wrap_err("failed to parse TOML")?;

    Ok(serde_path_to_error::deserialize(deserializer)?)
}

#[inline(always)]
pub fn write<'a, TData, TPath, TSys: FsWrite + Send + Sync>(
    path: TPath,
    data: &TData,
    sys: &TSys,
) -> Result<(), Error>
where
    TData: Serialize,
    TPath: AsRef<Path>,
{
    write_with_format(
        ext_to_format(path.as_ref().extension().unwrap_or_default())?,
        path,
        data,
        sys,
    )
}

pub fn write_with_format<'a, TData, TPath, TSys: FsWrite + Send + Sync>(
    format: Format,
    path: TPath,
    data: &TData,
    sys: &TSys,
) -> Result<(), Error>
where
    TData: Serialize,
    TPath: AsRef<Path>,
{
    let path = path.as_ref();
    let content = to_vec(data, format)?;
    sys.fs_write(path, content)?;

    Ok(())
}

#[inline(always)]
pub async fn write_async<'a, TData, TPath, TSys: FsWriteAsync + Send + Sync>(
    path: TPath,
    data: &TData,
    sys: &TSys,
) -> Result<(), Error>
where
    TData: Serialize,
    TPath: AsRef<Path>,
{
    write_with_format_async(
        ext_to_format(path.as_ref().extension().unwrap_or_default())?,
        path,
        data,
        sys,
    )
    .await
}

pub async fn write_with_format_async<
    'a,
    TData,
    TPath,
    TSys: FsWriteAsync + Send + Sync,
>(
    format: Format,
    path: TPath,
    data: &TData,
    sys: &TSys,
) -> Result<(), Error>
where
    TData: Serialize,
    TPath: AsRef<Path>,
{
    let path = path.as_ref();
    let content = to_vec(data, format)?;
    sys.fs_write_async(path, content).await?;

    Ok(())
}

pub fn to_vec<TData>(data: &TData, format: Format) -> Result<Vec<u8>, Error>
where
    TData: Serialize,
{
    let mut buf = Vec::new();
    to_writer(&mut buf, data, format)?;
    Ok(buf)
}

pub fn to_writer<TData>(
    writer: &mut impl std::io::Write,
    data: &TData,
    format: Format,
) -> Result<(), Error>
where
    TData: Serialize,
{
    match format {
        Format::Yaml => {
            noyalib::to_writer(writer, data)?;
            Ok(())
        }
        Format::Json => {
            serde_json::to_writer_pretty(writer, data)?;
            Ok(())
        }
        Format::Toml => {
            let s = toml::to_string(data)?;
            writer.write_all(s.as_bytes())?;
            Ok(())
        }
        Format::Bin => {
            rmp_serde::encode::write(writer, data)?;
            Ok(())
        }
    }
}

pub fn ext_to_format(ext: &std::ffi::OsStr) -> Result<Format, Error> {
    match ext.to_string_lossy().as_ref() {
        "yaml" | "yml" => Ok(Format::Yaml),
        "json" => Ok(Format::Json),
        "toml" => Ok(Format::Toml),
        "bin" => Ok(Format::Bin),
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
    TomlDeserialize(#[from] serde_path_to_error::Error<toml::de::Error>),

    #[error(transparent)]
    TomlSerialize(#[from] toml::ser::Error),

    #[error(transparent)]
    YmlDeserialize(#[from] serde_path_to_error::Error<noyalib::Error>),

    #[error(transparent)]
    YmlSerialize(#[from] noyalib::Error),

    #[error(transparent)]
    BinDeserialize(
        #[from] serde_path_to_error::Error<rmp_serde::decode::Error>,
    ),

    #[error(transparent)]
    BinSeserialize(#[from] rmp_serde::encode::Error),

    #[error(transparent)]
    JsonDeserialize(#[from] serde_path_to_error::Error<serde_json::Error>),

    #[error(transparent)]
    JsonSerialize(#[from] serde_json::Error),

    #[error(transparent)]
    Utf8(#[from] std::str::Utf8Error),

    #[error(transparent)]
    Unknown(#[from] eyre::Report),
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
