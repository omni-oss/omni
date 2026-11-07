use std::{
    io::{self, Cursor},
    path::Path,
};

use serde::{Deserialize, Serialize};
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use system_traits::{FsRead, FsReadAsync, FsWrite, FsWriteAsync};
use thiserror::Error;

omni_codes::prefixes!("file_data");

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

pub fn read<TData, TPath, TSys: FsRead + Send + Sync>(
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

pub fn read_with_format<TData, TPath, TSys: FsRead + Send + Sync>(
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
            let value = noyalib::from_reader(reader).map_err(|e| {
                let location =
                    e.location().map_or((0, 0), |l| (l.line(), l.column()));
                parse_failed("yaml", None, location)
            })?;
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
    let utf8_str = std::str::from_utf8(data)?;
    let deserializer = toml::Deserializer::parse(utf8_str).map_err(|e| {
        parse_failed("toml", None, toml_location(utf8_str, e.span()))
    })?;

    Ok(serde_path_to_error::deserialize(deserializer).map_err(|e| {
        parse_failed(
            "toml",
            path_of(&e),
            toml_location(utf8_str, e.inner().span()),
        )
    })?)
}

fn path_of<E>(error: &serde_path_to_error::Error<E>) -> Option<String> {
    let path = error.path().to_string();
    (path != ".").then_some(path)
}

fn toml_location(
    input: &str,
    span: Option<std::ops::Range<usize>>,
) -> (usize, usize) {
    let Some(before) = span.and_then(|s| input.get(..s.start)) else {
        return (0, 0);
    };
    let line = before.matches('\n').count() + 1;
    let column = before
        .rsplit('\n')
        .next()
        .unwrap_or_default()
        .chars()
        .count()
        + 1;
    (line, column)
}

/// The summary carries the format, path and location only: the parser's own
/// message can quote the offending value or line of the file.
fn parse_failed(
    format: &'static str,
    path: Option<String>,
    (line, column): (usize, usize),
) -> ErrorInner {
    let mut message = format!("failed to parse {} file", format.to_uppercase());
    if let Some(path) = &path {
        message.push_str(&format!(" at `{path}`"));
    }
    if line > 0 {
        message.push_str(&format!(" (line {line}, column {column})"));
    }
    ErrorInner::ParseFailed {
        format,
        path,
        line: (line > 0).then_some(line),
        column: (column > 0).then_some(column),
        message,
    }
}

#[inline(always)]
pub fn write<TData, TPath, TSys: FsWrite + Send + Sync>(
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

pub fn write_with_format<TData, TPath, TSys: FsWrite + Send + Sync>(
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
pub async fn write_async<TData, TPath, TSys: FsWriteAsync + Send + Sync>(
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

#[derive(Error, Debug, omni_diagnostic::Diagnostic)]
#[error(transparent)]
#[diagnostic(forward)]
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

#[derive(Error, Debug, EnumDiscriminants, omni_diagnostic::Diagnostic)]
#[strum_discriminants(vis(pub), name(ErrorKind))]
enum ErrorInner {
    /// The file extension does not name a supported data format.
    #[error("tried to load an unsupported file extension: {0}")]
    #[diagnostic(
        code = "file_data.unsupported_extension",
        hint = "rename the file to use one of the supported extensions: yaml, yml, json, toml or bin"
    )]
    UnsupportedFileExtension(#[diagnostic(detail = "extension")] String),

    #[error(transparent)]
    #[diagnostic(skip)]
    Io(#[from] io::Error),

    /// A YAML, JSON or TOML file is not valid at the reported location.
    #[error("{message}")]
    #[diagnostic(
        code = "file_data.parse_failed",
        hint = "fix the {format} syntax or the value at the reported location"
    )]
    ParseFailed {
        #[diagnostic(detail)]
        format: &'static str,
        #[diagnostic(detail)]
        path: Option<String>,
        #[diagnostic(detail)]
        line: Option<usize>,
        #[diagnostic(detail)]
        column: Option<usize>,
        message: String,
    },

    #[error(transparent)]
    #[diagnostic(skip)]
    TomlSerialize(#[from] toml::ser::Error),

    #[error(transparent)]
    #[diagnostic(skip)]
    YmlSerialize(#[from] noyalib::Error),

    #[error(transparent)]
    #[diagnostic(skip)]
    BinDeserialize(
        #[from] serde_path_to_error::Error<rmp_serde::decode::Error>,
    ),

    #[error(transparent)]
    #[diagnostic(skip)]
    BinSeserialize(#[from] rmp_serde::encode::Error),

    #[error(transparent)]
    #[diagnostic(skip)]
    JsonSerialize(#[from] serde_json::Error),

    #[error(transparent)]
    #[diagnostic(skip)]
    Utf8(#[from] std::str::Utf8Error),

    #[error(transparent)]
    #[diagnostic(delegate)]
    Unknown(#[from] eyre::Report),
}

impl From<serde_path_to_error::Error<serde_json::Error>> for ErrorInner {
    fn from(error: serde_path_to_error::Error<serde_json::Error>) -> Self {
        let inner = error.inner();
        parse_failed("json", path_of(&error), (inner.line(), inner.column()))
    }
}

impl From<serde_path_to_error::Error<noyalib::Error>> for ErrorInner {
    fn from(error: serde_path_to_error::Error<noyalib::Error>) -> Self {
        let location = error
            .inner()
            .location()
            .map_or((0, 0), |l| (l.line(), l.column()));
        parse_failed("yaml", path_of(&error), location)
    }
}

#[cfg(test)]
mod diagnostic_tests;

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
