//! File-system services exposed over Bridge RPC.
//!
//! These services back the `FileSystem` interface exposed in JS by
//! `packages/bridge-rpc-services/src/dry-run-system.ts`. Each service is
//! generic over a system type `S` that implements the relevant `*_async`
//! traits from [`system_traits`], so they can be wired up against either
//! `RealSys` (real, mutating implementation) or
//! [`DryRunSys`](omni_generator::DryRunSys).
//!
//! Wire conventions
//! -----------------
//!
//! - Trivial parameters (paths, flags, small option bags) live in the
//!   **request** headers under the single `parameters` key.
//! - Structured response payloads live in the **response** headers under
//!   the single `returns` key.
//! - Both are encoded directly as MessagePack values by the protocol layer;
//!   the services never re-serialize them as JSON.
//! - Bulk content (file bytes, file text) lives in the **body**, split
//!   into [`MAX_CHUNK_SIZE`](super::common::MAX_CHUNK_SIZE) chunks where
//!   necessary.
//!
//! The recommended path namespace is `"/fs/<kebab-case-method>"`, but the
//! services themselves do not enforce any particular routing.
use std::{path::PathBuf, sync::Arc};

use async_trait::async_trait;
use bridge_rpc_core::{
    service::{Service, ServiceContext},
    service_error::ServiceError,
};
use serde::{Deserialize, Serialize};
use system_traits::{
    BaseFsAppendAsync, BaseFsCopyAsync, BaseFsMetadataAsync, BaseFsReadAsync,
    BaseFsReadDirAsync, BaseFsRemoveDirAllAsync, BaseFsRemoveDirAsync,
    BaseFsRemoveFileAsync, BaseFsRenameAsync, BaseFsWriteAsync,
    CreateDirOptions, FileType, FsAppendAsync as _, FsCopyAsync as _,
    FsCreateDirAsync as _, FsMetadataAsync as _, FsMetadataValue,
    FsReadAsync as _, FsReadDirAsync as _, FsRemoveDirAllAsync as _,
    FsRemoveDirAsync as _, FsRemoveFileAsync as _, FsRenameAsync as _,
    FsWriteAsync as _,
};

use super::common::{
    read_full_body, read_parameters, respond_empty, respond_with_body,
    respond_with_returns,
};

// ---------------------------------------------------------------------------
// Request parameter schemas
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Debug, Clone)]
struct PathParams {
    pub path: PathBuf,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, Copy)]
struct CreateDirectoryOptions {
    #[serde(default)]
    pub recursive: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct CreateDirectoryParams {
    pub path: PathBuf,
    #[serde(default)]
    pub options: CreateDirectoryOptions,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, Copy)]
struct RemoveOptions {
    #[serde(default)]
    pub recursive: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct RemoveParams {
    pub path: PathBuf,
    #[serde(default)]
    pub options: RemoveOptions,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct RenameParams {
    pub old_path: PathBuf,
    pub new_path: PathBuf,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, Copy)]
#[allow(dead_code)] // forward-compatibility flags not yet honoured
struct CopyOptions {
    #[serde(default)]
    pub overwrite: bool,
    #[serde(default)]
    pub recursive: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct CopyParams {
    pub src: PathBuf,
    pub dest: PathBuf,
    #[serde(default)]
    pub options: CopyOptions,
}

// ---------------------------------------------------------------------------
// Response parameter schemas
// ---------------------------------------------------------------------------

/// Response payload used by all boolean-returning services
/// (`pathExists`, `isFile`, `isDirectory`, `isSymbolicLink`).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoolResponse {
    pub value: bool,
}

/// Response payload for `readDirectory`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ReadDirectoryResponse {
    pub entries: Vec<String>,
}

/// Response payload for `stat`.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatResponse {
    pub is_file: bool,
    pub is_directory: bool,
    pub is_symbolic_link: bool,
    pub size: u64,
    /// Last-modified time, encoded as milliseconds since the Unix epoch.
    pub mtime_ms: i64,
}

fn to_stat_response<M: FsMetadataValue>(metadata: &M) -> StatResponse {
    let file_type = metadata.file_type();
    let mtime_ms = metadata
        .modified()
        .ok()
        .and_then(|st| st.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    StatResponse {
        is_file: file_type == FileType::File,
        is_directory: file_type == FileType::Dir,
        is_symbolic_link: file_type == FileType::Symlink,
        size: metadata.len(),
        mtime_ms,
    }
}

// ---------------------------------------------------------------------------
// Generic service shell
// ---------------------------------------------------------------------------

macro_rules! define_service {
    ($(#[$attr:meta])* $name:ident) => {
        $(#[$attr])*
        #[derive(Debug)]
        pub struct $name<S> {
            sys: Arc<S>,
        }

        impl<S> $name<S> {
            /// Creates a new service backed by the provided system handle.
            pub fn new(sys: Arc<S>) -> Self {
                Self { sys }
            }

            /// Borrow the underlying sys handle.
            pub fn sys(&self) -> &Arc<S> {
                &self.sys
            }
        }

        impl<S> Clone for $name<S> {
            fn clone(&self) -> Self {
                Self {
                    sys: self.sys.clone(),
                }
            }
        }
    };
}

define_service!(
    /// Backs `FileSystem.readFileAsString(path)`.
    ///
    /// Request: `parameters = { path }`. Body: empty.
    /// Response: body holds the UTF-8 encoded file contents (chunked).
    ReadFileAsStringService
);
define_service!(
    /// Backs `FileSystem.readFileAsBytes(path)`.
    ///
    /// Request: `parameters = { path }`. Body: empty.
    /// Response: body holds the raw file contents (chunked).
    ReadFileAsBytesService
);
define_service!(
    /// Backs `FileSystem.writeStringToFile(path, content)`.
    ///
    /// Request: `parameters = { path }`. Body: UTF-8 content (chunked).
    /// Response: empty.
    WriteStringToFileService
);
define_service!(
    /// Backs `FileSystem.writeBytesToFile(path, content)`.
    ///
    /// Request: `parameters = { path }`. Body: raw bytes (chunked).
    /// Response: empty.
    WriteBytesToFileService
);
define_service!(
    /// Backs `FileSystem.pathExists(path)`.
    ///
    /// Request: `parameters = { path }`. Response: `parameters = { value }`.
    PathExistsService
);
define_service!(
    /// Backs `FileSystem.createDirectory(path, options)`.
    ///
    /// Request: `parameters = { path, options? }`.
    CreateDirectoryService
);
define_service!(
    /// Backs `FileSystem.readDirectory(path)`.
    ///
    /// Request: `parameters = { path }`.
    /// Response: `parameters = { entries }`.
    ReadDirectoryService
);
define_service!(
    /// Backs `FileSystem.remove(path, options)`.
    ///
    /// Request: `parameters = { path, options? }`.
    RemoveService
);
define_service!(
    /// Backs `FileSystem.rename(oldPath, newPath)`.
    ///
    /// Request: `parameters = { old_path, new_path }`.
    RenameService
);
define_service!(
    /// Backs `FileSystem.stat(path)`.
    ///
    /// Request: `parameters = { path }`.
    /// Response: `parameters = StatResponse`.
    StatService
);
define_service!(
    /// Backs `FileSystem.isFile(path)`.
    IsFileService
);
define_service!(
    /// Backs `FileSystem.isDirectory(path)`.
    IsDirectoryService
);
define_service!(
    /// Backs `FileSystem.isSymbolicLink(path)`.
    IsSymbolicLinkService
);
define_service!(
    /// Backs `FileSystem.copy(src, dest, options)`.
    ///
    /// Request: `parameters = { src, dest, options? }`.
    CopyService
);
define_service!(
    /// Backs `FileSystem.appendStringToFile(path, content)`.
    ///
    /// Request: `parameters = { path }`. Body: UTF-8 content (chunked).
    AppendStringToFileService
);

// ---------------------------------------------------------------------------
// Service implementations
// ---------------------------------------------------------------------------

#[async_trait]
impl<S> Service for ReadFileAsStringService<S>
where
    S: BaseFsReadAsync + Send + Sync + 'static,
{
    async fn run(&self, context: ServiceContext) -> Result<(), ServiceError> {
        let ServiceContext {
            request, response, ..
        } = context;
        let params = read_parameters::<PathParams>(request.headers())?;

        let bytes = self
            .sys
            .fs_read_async(&params.path)
            .await
            .map_err(ServiceError::custom_error)?;

        // Validate UTF-8 here so callers see a structured error rather than
        // an arbitrary decode failure on the JS side.
        std::str::from_utf8(&bytes).map_err(ServiceError::custom_error)?;

        respond_with_body(response, &bytes).await
    }
}

#[async_trait]
impl<S> Service for ReadFileAsBytesService<S>
where
    S: BaseFsReadAsync + Send + Sync + 'static,
{
    async fn run(&self, context: ServiceContext) -> Result<(), ServiceError> {
        let ServiceContext {
            request, response, ..
        } = context;
        let params = read_parameters::<PathParams>(request.headers())?;

        let bytes = self
            .sys
            .fs_read_async(&params.path)
            .await
            .map_err(ServiceError::custom_error)?;

        respond_with_body(response, &bytes).await
    }
}

#[async_trait]
impl<S> Service for WriteStringToFileService<S>
where
    S: BaseFsWriteAsync + Send + Sync + 'static,
{
    async fn run(&self, context: ServiceContext) -> Result<(), ServiceError> {
        let ServiceContext {
            request, response, ..
        } = context;
        let params = read_parameters::<PathParams>(request.headers())?;

        let body = read_full_body(request).await?;
        // Reject non-UTF-8 bodies eagerly: callers that need raw byte writes
        // should use `WriteBytesToFileService`.
        std::str::from_utf8(&body).map_err(ServiceError::custom_error)?;

        self.sys
            .fs_write_async(&params.path, &body)
            .await
            .map_err(ServiceError::custom_error)?;

        respond_empty(response).await
    }
}

#[async_trait]
impl<S> Service for WriteBytesToFileService<S>
where
    S: BaseFsWriteAsync + Send + Sync + 'static,
{
    async fn run(&self, context: ServiceContext) -> Result<(), ServiceError> {
        let ServiceContext {
            request, response, ..
        } = context;
        let params = read_parameters::<PathParams>(request.headers())?;

        let body = read_full_body(request).await?;

        self.sys
            .fs_write_async(&params.path, &body)
            .await
            .map_err(ServiceError::custom_error)?;

        respond_empty(response).await
    }
}

#[async_trait]
impl<S> Service for PathExistsService<S>
where
    S: BaseFsMetadataAsync + Send + Sync + 'static,
{
    async fn run(&self, context: ServiceContext) -> Result<(), ServiceError> {
        let ServiceContext {
            request, response, ..
        } = context;
        let params = read_parameters::<PathParams>(request.headers())?;

        let exists = self
            .sys
            .fs_exists_async(&params.path)
            .await
            .map_err(ServiceError::custom_error)?;

        respond_with_returns(response, &BoolResponse { value: exists }).await
    }
}

#[async_trait]
impl<S> Service for CreateDirectoryService<S>
where
    S: system_traits::BaseFsCreateDirAsync + Send + Sync + 'static,
{
    async fn run(&self, context: ServiceContext) -> Result<(), ServiceError> {
        let ServiceContext {
            request, response, ..
        } = context;
        let params =
            read_parameters::<CreateDirectoryParams>(request.headers())?;

        let mut options = CreateDirOptions::new();
        options.recursive = params.options.recursive;

        self.sys
            .fs_create_dir_async(&params.path, &options)
            .await
            .map_err(ServiceError::custom_error)?;

        respond_empty(response).await
    }
}

#[async_trait]
impl<S> Service for ReadDirectoryService<S>
where
    S: BaseFsReadDirAsync + Send + Sync + 'static,
{
    async fn run(&self, context: ServiceContext) -> Result<(), ServiceError> {
        let ServiceContext {
            request, response, ..
        } = context;
        let params = read_parameters::<PathParams>(request.headers())?;

        let entries = self
            .sys
            .fs_read_dir_async(&params.path)
            .await
            .map_err(ServiceError::custom_error)?;

        let entries: Vec<String> = entries
            .into_iter()
            .map(|p| {
                p.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| p.to_string_lossy().into_owned())
            })
            .collect();

        respond_with_returns(response, &ReadDirectoryResponse { entries }).await
    }
}

#[async_trait]
impl<S> Service for RemoveService<S>
where
    S: BaseFsMetadataAsync
        + BaseFsRemoveDirAsync
        + BaseFsRemoveDirAllAsync
        + BaseFsRemoveFileAsync
        + Send
        + Sync
        + 'static,
    <S as BaseFsMetadataAsync>::Metadata: Send,
{
    async fn run(&self, context: ServiceContext) -> Result<(), ServiceError> {
        let ServiceContext {
            request, response, ..
        } = context;
        let params = read_parameters::<RemoveParams>(request.headers())?;

        // Distinguish file vs directory removal so we can pick the
        // appropriate trait method. We treat "not found" as a noop here to
        // mirror the expected `Promise<void>` semantics on the JS side.
        let file_type = {
            match self.sys.fs_symlink_metadata_async(&params.path).await {
                Ok(m) => m.file_type(),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    return respond_empty(response).await;
                }
                Err(err) => {
                    return Err(ServiceError::custom_error(err));
                }
            }
        };

        if file_type == FileType::Dir {
            if params.options.recursive {
                self.sys
                    .fs_remove_dir_all_async(&params.path)
                    .await
                    .map_err(ServiceError::custom_error)?;
            } else {
                self.sys
                    .fs_remove_dir_async(&params.path)
                    .await
                    .map_err(ServiceError::custom_error)?;
            }
        } else {
            self.sys
                .fs_remove_file_async(&params.path)
                .await
                .map_err(ServiceError::custom_error)?;
        }

        respond_empty(response).await
    }
}

#[async_trait]
impl<S> Service for RenameService<S>
where
    S: BaseFsRenameAsync + Send + Sync + 'static,
{
    async fn run(&self, context: ServiceContext) -> Result<(), ServiceError> {
        let ServiceContext {
            request, response, ..
        } = context;
        let params = read_parameters::<RenameParams>(request.headers())?;

        self.sys
            .fs_rename_async(&params.old_path, &params.new_path)
            .await
            .map_err(ServiceError::custom_error)?;

        respond_empty(response).await
    }
}

#[async_trait]
impl<S> Service for StatService<S>
where
    S: BaseFsMetadataAsync + Send + Sync + 'static,
    <S as BaseFsMetadataAsync>::Metadata: Send,
{
    async fn run(&self, context: ServiceContext) -> Result<(), ServiceError> {
        let ServiceContext {
            request, response, ..
        } = context;
        let params = read_parameters::<PathParams>(request.headers())?;

        let stat = {
            let metadata = self
                .sys
                .fs_metadata_async(&params.path)
                .await
                .map_err(ServiceError::custom_error)?;
            to_stat_response(&metadata)
        };

        respond_with_returns(response, &stat).await
    }
}

#[async_trait]
impl<S> Service for IsFileService<S>
where
    S: BaseFsMetadataAsync + Send + Sync + 'static,
{
    async fn run(&self, context: ServiceContext) -> Result<(), ServiceError> {
        let ServiceContext {
            request, response, ..
        } = context;
        let params = read_parameters::<PathParams>(request.headers())?;

        let value = self
            .sys
            .fs_is_file_async(&params.path)
            .await
            .map_err(ServiceError::custom_error)?;

        respond_with_returns(response, &BoolResponse { value }).await
    }
}

#[async_trait]
impl<S> Service for IsDirectoryService<S>
where
    S: BaseFsMetadataAsync + Send + Sync + 'static,
{
    async fn run(&self, context: ServiceContext) -> Result<(), ServiceError> {
        let ServiceContext {
            request, response, ..
        } = context;
        let params = read_parameters::<PathParams>(request.headers())?;

        let value = self
            .sys
            .fs_is_dir_async(&params.path)
            .await
            .map_err(ServiceError::custom_error)?;

        respond_with_returns(response, &BoolResponse { value }).await
    }
}

#[async_trait]
impl<S> Service for IsSymbolicLinkService<S>
where
    S: BaseFsMetadataAsync + Send + Sync + 'static,
{
    async fn run(&self, context: ServiceContext) -> Result<(), ServiceError> {
        let ServiceContext {
            request, response, ..
        } = context;
        let params = read_parameters::<PathParams>(request.headers())?;

        let value = self
            .sys
            .fs_is_symlink_async(&params.path)
            .await
            .map_err(ServiceError::custom_error)?;

        respond_with_returns(response, &BoolResponse { value }).await
    }
}

#[async_trait]
impl<S> Service for CopyService<S>
where
    S: BaseFsCopyAsync + Send + Sync + 'static,
{
    async fn run(&self, context: ServiceContext) -> Result<(), ServiceError> {
        let ServiceContext {
            request, response, ..
        } = context;
        let params = read_parameters::<CopyParams>(request.headers())?;

        // The current `BaseFsCopyAsync` is a single-file copy; the
        // `recursive` and `overwrite` flags are accepted from the wire for
        // forward-compatibility with directory copies but are not yet
        // honoured beyond the underlying fs semantics.
        let _ = params.options;

        self.sys
            .fs_copy_async(&params.src, &params.dest)
            .await
            .map_err(ServiceError::custom_error)?;

        respond_empty(response).await
    }
}

#[async_trait]
impl<S> Service for AppendStringToFileService<S>
where
    S: BaseFsAppendAsync + Send + Sync + 'static,
{
    async fn run(&self, context: ServiceContext) -> Result<(), ServiceError> {
        let ServiceContext {
            request, response, ..
        } = context;
        let params = read_parameters::<PathParams>(request.headers())?;

        let body = read_full_body(request).await?;
        std::str::from_utf8(&body).map_err(ServiceError::custom_error)?;

        self.sys
            .fs_append_async(&params.path, &body)
            .await
            .map_err(ServiceError::custom_error)?;

        respond_empty(response).await
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bridge_rpc_core::{ResponseStatusCode, service::Service};
    use system_traits::impls::RealSys;
    use tempfile::TempDir;

    use super::*;
    use crate::services::{
        common::{RETURNS_HEADER, encode_parameters},
        test_harness::ServiceContextBuilder,
    };

    fn real_sys() -> Arc<RealSys> {
        Arc::new(RealSys::default())
    }

    /// Reads the `returns` header from a response into the requested type.
    fn read_response_returns<T>(headers: &Option<bridge_rpc_core::DynMap>) -> T
    where
        T: serde::de::DeserializeOwned,
    {
        let value = headers
            .as_ref()
            .and_then(|h| h.get_raw(RETURNS_HEADER))
            .expect("response should include the `returns` header")
            .clone();
        rmpv::ext::from_value::<T>(value)
            .expect("response returns should decode")
    }

    fn params_for<T: serde::Serialize>(value: &T) -> bridge_rpc_core::DynMap {
        encode_parameters(value).expect("encoding parameters should succeed")
    }

    #[tokio::test]
    async fn read_file_as_string_returns_file_contents_in_body() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("hello.txt");
        std::fs::write(&path, "hello world").unwrap();

        let service = ReadFileAsStringService::new(real_sys());

        let (ctx, awaiter) =
            ServiceContextBuilder::new("/fs/read-file-as-string")
                .with_headers(params_for(&PathParams { path }))
                .build()
                .await;

        service.run(ctx).await.expect("service should succeed");

        let response = awaiter.wait().await;
        assert_eq!(response.status, ResponseStatusCode::SUCCESS);
        assert_eq!(response.body, b"hello world");
    }

    #[tokio::test]
    async fn read_file_as_bytes_returns_raw_bytes() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("blob.bin");
        let raw: Vec<u8> = vec![0u8, 1, 2, 3, 254, 255];
        std::fs::write(&path, &raw).unwrap();

        let service = ReadFileAsBytesService::new(real_sys());

        let (ctx, awaiter) =
            ServiceContextBuilder::new("/fs/read-file-as-bytes")
                .with_headers(params_for(&PathParams { path }))
                .build()
                .await;

        service.run(ctx).await.expect("service should succeed");

        let response = awaiter.wait().await;
        assert_eq!(response.status, ResponseStatusCode::SUCCESS);
        assert_eq!(response.body, raw);
    }

    #[tokio::test]
    async fn read_file_chunks_large_payloads() {
        // The body should be split into chunks of at most MAX_CHUNK_SIZE.
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("big.bin");

        // Use a content size that strictly exceeds MAX_CHUNK_SIZE so we get
        // multiple chunks back.
        let len = super::super::common::MAX_CHUNK_SIZE * 2 + 17;
        let payload: Vec<u8> = (0..len).map(|i| i as u8).collect();
        std::fs::write(&path, &payload).unwrap();

        let service = ReadFileAsBytesService::new(real_sys());

        let (ctx, mut awaiter) =
            ServiceContextBuilder::new("/fs/read-file-as-bytes")
                .with_headers(params_for(&PathParams { path }))
                .build()
                .await;

        service.run(ctx).await.expect("service should succeed");

        // After `run` returns, every response frame has been sent into the
        // test channel - we just need to drain it. Counting `BodyChunk`
        // frames lets us assert that chunking actually happened.
        use bridge_rpc_core::frame::Frame;
        let mut chunk_count = 0usize;
        let mut total = Vec::new();
        let mut got_end = false;
        for _ in 0..1024 {
            if let Some(frame) = awaiter.try_next_frame() {
                match frame {
                    Frame::ResponseStart(_) => {}
                    Frame::ResponseBodyChunk(chunk) => {
                        chunk_count += 1;
                        total.extend_from_slice(&chunk.chunk);
                        assert!(
                            chunk.chunk.len()
                                <= super::super::common::MAX_CHUNK_SIZE,
                            "each chunk must respect MAX_CHUNK_SIZE"
                        );
                    }
                    Frame::ResponseEnd(_) => {
                        got_end = true;
                        break;
                    }
                    other => panic!("unexpected frame: {other:?}"),
                }
            } else if awaiter.is_drained() {
                break;
            } else {
                tokio::task::yield_now().await;
            }
        }

        assert!(got_end, "response should end with a `ResponseEnd` frame");
        assert!(
            chunk_count >= 3,
            "expected the body to be split into at least 3 chunks, \
             got {chunk_count}"
        );
        assert_eq!(total, payload);
    }

    #[tokio::test]
    async fn write_string_to_file_writes_body_contents() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("out.txt");

        let service = WriteStringToFileService::new(real_sys());

        let (ctx, awaiter) =
            ServiceContextBuilder::new("/fs/write-string-to-file")
                .with_headers(params_for(&PathParams { path: path.clone() }))
                .with_body_bytes(b"data".to_vec())
                .build()
                .await;

        service.run(ctx).await.expect("service should succeed");

        let response = awaiter.wait().await;
        assert_eq!(response.status, ResponseStatusCode::SUCCESS);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "data");
    }

    #[tokio::test]
    async fn write_bytes_to_file_writes_body_contents() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("out.bin");

        let service = WriteBytesToFileService::new(real_sys());

        let (ctx, awaiter) =
            ServiceContextBuilder::new("/fs/write-bytes-to-file")
                .with_headers(params_for(&PathParams { path: path.clone() }))
                .with_body_bytes(vec![1u8, 2, 3, 4, 5])
                .build()
                .await;

        service.run(ctx).await.expect("service should succeed");

        let response = awaiter.wait().await;
        assert_eq!(response.status, ResponseStatusCode::SUCCESS);
        assert_eq!(std::fs::read(&path).unwrap(), vec![1u8, 2, 3, 4, 5]);
    }

    #[tokio::test]
    async fn path_exists_reports_existence() {
        let tmp = TempDir::new().unwrap();
        let existing = tmp.path().join("yes");
        std::fs::write(&existing, b"x").unwrap();

        let service = PathExistsService::new(real_sys());

        let (ctx, awaiter) = ServiceContextBuilder::new("/fs/path-exists")
            .with_headers(params_for(&PathParams { path: existing }))
            .build()
            .await;

        service.run(ctx).await.expect("service should succeed");

        let response = awaiter.wait().await;
        let parsed: BoolResponse = read_response_returns(&response.headers);
        assert!(parsed.value);

        // Now check a non-existent path.
        let missing = tmp.path().join("no");
        let service = PathExistsService::new(real_sys());
        let (ctx, awaiter) = ServiceContextBuilder::new("/fs/path-exists")
            .with_headers(params_for(&PathParams { path: missing }))
            .build()
            .await;
        service.run(ctx).await.expect("service should succeed");
        let response = awaiter.wait().await;
        let parsed: BoolResponse = read_response_returns(&response.headers);
        assert!(!parsed.value);
    }

    #[tokio::test]
    async fn create_directory_creates_nested_dirs_when_recursive() {
        let tmp = TempDir::new().unwrap();
        let nested = tmp.path().join("a").join("b").join("c");

        let service = CreateDirectoryService::new(real_sys());

        let (ctx, awaiter) = ServiceContextBuilder::new("/fs/create-directory")
            .with_headers(params_for(&CreateDirectoryParams {
                path: nested.clone(),
                options: CreateDirectoryOptions { recursive: true },
            }))
            .build()
            .await;

        service.run(ctx).await.expect("service should succeed");
        let response = awaiter.wait().await;
        assert_eq!(response.status, ResponseStatusCode::SUCCESS);

        assert!(nested.is_dir());
    }

    #[tokio::test]
    async fn read_directory_returns_entry_names() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.txt"), b"a").unwrap();
        std::fs::write(tmp.path().join("b.txt"), b"b").unwrap();

        let service = ReadDirectoryService::new(real_sys());
        let (ctx, awaiter) = ServiceContextBuilder::new("/fs/read-directory")
            .with_headers(params_for(&PathParams {
                path: tmp.path().to_path_buf(),
            }))
            .build()
            .await;

        service.run(ctx).await.expect("service should succeed");
        let response = awaiter.wait().await;
        let parsed: ReadDirectoryResponse =
            read_response_returns(&response.headers);
        let mut entries = parsed.entries;
        entries.sort();
        assert_eq!(entries, vec!["a.txt".to_string(), "b.txt".to_string()]);
    }

    #[tokio::test]
    async fn remove_deletes_file() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("gone.txt");
        std::fs::write(&target, b"bye").unwrap();

        let service = RemoveService::new(real_sys());
        let (ctx, awaiter) = ServiceContextBuilder::new("/fs/remove")
            .with_headers(params_for(&RemoveParams {
                path: target.clone(),
                options: RemoveOptions::default(),
            }))
            .build()
            .await;
        service.run(ctx).await.expect("service should succeed");
        let _ = awaiter.wait().await;

        assert!(!target.exists());
    }

    #[tokio::test]
    async fn remove_deletes_dir_recursively() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("d");
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub").join("f"), b"x").unwrap();

        let service = RemoveService::new(real_sys());
        let (ctx, awaiter) = ServiceContextBuilder::new("/fs/remove")
            .with_headers(params_for(&RemoveParams {
                path: dir.clone(),
                options: RemoveOptions { recursive: true },
            }))
            .build()
            .await;
        service.run(ctx).await.expect("service should succeed");
        let _ = awaiter.wait().await;

        assert!(!dir.exists());
    }

    #[tokio::test]
    async fn rename_moves_file() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("from.txt");
        let dst = tmp.path().join("to.txt");
        std::fs::write(&src, b"hi").unwrap();

        let service = RenameService::new(real_sys());
        let (ctx, awaiter) = ServiceContextBuilder::new("/fs/rename")
            .with_headers(params_for(&RenameParams {
                old_path: src.clone(),
                new_path: dst.clone(),
            }))
            .build()
            .await;
        service.run(ctx).await.expect("service should succeed");
        let _ = awaiter.wait().await;

        assert!(!src.exists());
        assert_eq!(std::fs::read_to_string(&dst).unwrap(), "hi");
    }

    #[tokio::test]
    async fn stat_reports_file_metadata() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("a.txt");
        std::fs::write(&path, b"abcdef").unwrap();

        let service = StatService::new(real_sys());
        let (ctx, awaiter) = ServiceContextBuilder::new("/fs/stat")
            .with_headers(params_for(&PathParams { path }))
            .build()
            .await;
        service.run(ctx).await.expect("service should succeed");
        let response = awaiter.wait().await;
        let parsed: StatResponse = read_response_returns(&response.headers);

        assert!(parsed.is_file);
        assert!(!parsed.is_directory);
        assert!(!parsed.is_symbolic_link);
        assert_eq!(parsed.size, 6);
    }

    #[tokio::test]
    async fn is_file_returns_true_for_regular_files() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("a.txt");
        std::fs::write(&path, b"x").unwrap();

        let service = IsFileService::new(real_sys());
        let (ctx, awaiter) = ServiceContextBuilder::new("/fs/is-file")
            .with_headers(params_for(&PathParams { path }))
            .build()
            .await;
        service.run(ctx).await.expect("service should succeed");
        let response = awaiter.wait().await;
        let parsed: BoolResponse = read_response_returns(&response.headers);
        assert!(parsed.value);
    }

    #[tokio::test]
    async fn is_directory_returns_true_for_dirs() {
        let tmp = TempDir::new().unwrap();
        let service = IsDirectoryService::new(real_sys());
        let (ctx, awaiter) = ServiceContextBuilder::new("/fs/is-directory")
            .with_headers(params_for(&PathParams {
                path: tmp.path().to_path_buf(),
            }))
            .build()
            .await;
        service.run(ctx).await.expect("service should succeed");
        let response = awaiter.wait().await;
        let parsed: BoolResponse = read_response_returns(&response.headers);
        assert!(parsed.value);
    }

    #[tokio::test]
    async fn copy_copies_a_file() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src.txt");
        let dst = tmp.path().join("dst.txt");
        std::fs::write(&src, b"copied").unwrap();

        let service = CopyService::new(real_sys());
        let (ctx, awaiter) = ServiceContextBuilder::new("/fs/copy")
            .with_headers(params_for(&CopyParams {
                src: src.clone(),
                dest: dst.clone(),
                options: CopyOptions::default(),
            }))
            .build()
            .await;
        service.run(ctx).await.expect("service should succeed");
        let _ = awaiter.wait().await;

        assert_eq!(std::fs::read_to_string(&dst).unwrap(), "copied");
    }

    #[tokio::test]
    async fn append_string_to_file_appends_content() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("log.txt");
        std::fs::write(&path, b"line1\n").unwrap();

        let service = AppendStringToFileService::new(real_sys());
        let (ctx, awaiter) =
            ServiceContextBuilder::new("/fs/append-string-to-file")
                .with_headers(params_for(&PathParams { path: path.clone() }))
                .with_body_bytes(b"line2\n".to_vec())
                .build()
                .await;
        service.run(ctx).await.expect("service should succeed");
        let _ = awaiter.wait().await;

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "line1\nline2\n");
    }
}
