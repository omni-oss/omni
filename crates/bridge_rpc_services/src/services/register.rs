//! Convenience helpers for wiring all of this crate's services into a
//! [`bridge_rpc_router::Router`].
//!
//! The general approach is:
//!
//! 1. Construct an `Arc<S>` system handle (typically [`RealSys`] or the
//!    overlay [`DryRunSys`]).
//! 2. Optionally pick an [`ArgsProvider`] (defaults to [`StdArgsProvider`]).
//! 3. Pick a [`log::Log`] handle for the `/log` service (defaults to
//!    [`log::logger`]).
//! 4. Call [`register_services`] (or the granular `register_fs_services`
//!    / `register_proc_services` / `register_log_service` helpers) with a
//!    mutable reference to the router.
//!
//! Routes
//! ------
//!
//! By default the services are mounted under `/fs/<kebab-case-method>`
//! and `/proc/<kebab-case-method>`, with the log service at `/log`. Path
//! prefixes can be customised via [`RegisterServicesOptions`] when the
//! defaults are not appropriate.
//!
//! [`RealSys`]: system_traits::impls::RealSys
//! [`DryRunSys`]: omni_generator::DryRunSys
//! [`StdArgsProvider`]: super::proc::StdArgsProvider
//! [`ArgsProvider`]: super::proc::ArgsProvider
use std::sync::Arc;

use bridge_rpc_router::Router;
use log::Log;
use system_traits::{
    BaseEnvSetCurrentDirAsync, BaseFsAppendAsync, BaseFsCopyAsync,
    BaseFsCreateDirAsync, BaseFsMetadataAsync, BaseFsReadAsync,
    BaseFsReadDirAsync, BaseFsRemoveDirAllAsync, BaseFsRemoveDirAsync,
    BaseFsRemoveFileAsync, BaseFsRenameAsync, BaseFsWriteAsync,
    EnvCurrentDirAsync, EnvVars,
};

use super::{
    fs::{
        AppendStringToFileService, CopyService, CreateDirectoryService,
        IsDirectoryService, IsFileService, IsSymbolicLinkService,
        PathExistsService, ReadDirectoryService, ReadFileAsBytesService,
        ReadFileAsStringService, RemoveService, RenameService, StatService,
        WriteBytesToFileService, WriteStringToFileService,
    },
    log::LogService,
    proc::{
        ArgsProvider, ArgsService, CurrentDirService, EnvService,
        SetCurrentDirService, SnapshotService, StdArgsProvider,
    },
};

// ---------------------------------------------------------------------------
// Trait aliases
// ---------------------------------------------------------------------------

/// Aggregated bound that a system handle must satisfy in order to back the
/// full set of file-system services.
///
/// Implemented automatically for any type that implements all of the
/// underlying `*_async` traits. You should never need to implement this
/// trait manually.
pub trait FsSys:
    BaseFsReadAsync
    + BaseFsWriteAsync
    + BaseFsMetadataAsync
    + BaseFsCreateDirAsync
    + BaseFsReadDirAsync
    + BaseFsRemoveDirAsync
    + BaseFsRemoveDirAllAsync
    + BaseFsRemoveFileAsync
    + BaseFsRenameAsync
    + BaseFsCopyAsync
    + BaseFsAppendAsync
    + Send
    + Sync
    + 'static
where
    <Self as BaseFsMetadataAsync>::Metadata: Send,
{
}

impl<T> FsSys for T
where
    T: BaseFsReadAsync
        + BaseFsWriteAsync
        + BaseFsMetadataAsync
        + BaseFsCreateDirAsync
        + BaseFsReadDirAsync
        + BaseFsRemoveDirAsync
        + BaseFsRemoveDirAllAsync
        + BaseFsRemoveFileAsync
        + BaseFsRenameAsync
        + BaseFsCopyAsync
        + BaseFsAppendAsync
        + Send
        + Sync
        + 'static,
    <T as BaseFsMetadataAsync>::Metadata: Send,
{
}

/// Aggregated bound that a system handle must satisfy in order to back the
/// process services.
pub trait ProcSys:
    EnvCurrentDirAsync + BaseEnvSetCurrentDirAsync + EnvVars + Send + Sync + 'static
{
}

impl<T> ProcSys for T where
    T: EnvCurrentDirAsync
        + BaseEnvSetCurrentDirAsync
        + EnvVars
        + Send
        + Sync
        + 'static
{
}

// ---------------------------------------------------------------------------
// Path constants
// ---------------------------------------------------------------------------

/// Default prefix for the file-system services (`/fs`).
pub const DEFAULT_FS_PREFIX: &str = "/fs";
/// Default prefix for the process services (`/proc`).
pub const DEFAULT_PROC_PREFIX: &str = "/proc";
/// Default path for the log service (`/log`).
pub const DEFAULT_LOG_PATH: &str = "/log";

/// All file-system route names (relative to the FS prefix).
pub mod fs_routes {
    pub const READ_FILE_AS_STRING: &str = "/read-file-as-string";
    pub const READ_FILE_AS_BYTES: &str = "/read-file-as-bytes";
    pub const WRITE_STRING_TO_FILE: &str = "/write-string-to-file";
    pub const WRITE_BYTES_TO_FILE: &str = "/write-bytes-to-file";
    pub const PATH_EXISTS: &str = "/path-exists";
    pub const CREATE_DIRECTORY: &str = "/create-directory";
    pub const READ_DIRECTORY: &str = "/read-directory";
    pub const REMOVE: &str = "/remove";
    pub const RENAME: &str = "/rename";
    pub const STAT: &str = "/stat";
    pub const IS_FILE: &str = "/is-file";
    pub const IS_DIRECTORY: &str = "/is-directory";
    pub const IS_SYMBOLIC_LINK: &str = "/is-symbolic-link";
    pub const COPY: &str = "/copy";
    pub const APPEND_STRING_TO_FILE: &str = "/append-string-to-file";
}

/// All process route names (relative to the proc prefix).
pub mod proc_routes {
    pub const CURRENT_DIR: &str = "/current-dir";
    pub const SET_CURRENT_DIR: &str = "/set-current-dir";
    pub const ARGS: &str = "/args";
    pub const ENV: &str = "/env";
    pub const SNAPSHOT: &str = "/snapshot";
}

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

/// Configuration knobs for [`register_services`].
#[derive(Debug, Clone)]
pub struct RegisterServicesOptions {
    /// Path prefix prepended to every file-system route. Defaults to
    /// [`DEFAULT_FS_PREFIX`].
    pub fs_prefix: String,
    /// Path prefix prepended to every process route. Defaults to
    /// [`DEFAULT_PROC_PREFIX`].
    pub proc_prefix: String,
    /// Path of the log service. Defaults to [`DEFAULT_LOG_PATH`].
    pub log_path: String,
}

impl Default for RegisterServicesOptions {
    fn default() -> Self {
        Self {
            fs_prefix: DEFAULT_FS_PREFIX.to_string(),
            proc_prefix: DEFAULT_PROC_PREFIX.to_string(),
            log_path: DEFAULT_LOG_PATH.to_string(),
        }
    }
}

impl RegisterServicesOptions {
    /// Convenience constructor returning the default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Override the FS prefix (e.g. `"/api/fs"`).
    pub fn with_fs_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.fs_prefix = prefix.into();
        self
    }

    /// Override the process prefix.
    pub fn with_proc_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.proc_prefix = prefix.into();
        self
    }

    /// Override the log service path.
    pub fn with_log_path(mut self, path: impl Into<String>) -> Self {
        self.log_path = path.into();
        self
    }
}

fn join(prefix: &str, route: &str) -> String {
    let prefix = prefix.trim_end_matches('/');
    let route = route.trim_start_matches('/');
    format!("{prefix}/{route}")
}

// ---------------------------------------------------------------------------
// Registration helpers
// ---------------------------------------------------------------------------

/// Registers every file-system service against the given router under
/// `prefix`.
pub fn register_fs_services<S>(router: &mut Router, sys: Arc<S>, prefix: &str)
where
    S: FsSys,
    <S as BaseFsMetadataAsync>::Metadata: Send,
{
    use fs_routes as r;

    router.add_service(
        join(prefix, r::READ_FILE_AS_STRING),
        ReadFileAsStringService::new(sys.clone()),
    );
    router.add_service(
        join(prefix, r::READ_FILE_AS_BYTES),
        ReadFileAsBytesService::new(sys.clone()),
    );
    router.add_service(
        join(prefix, r::WRITE_STRING_TO_FILE),
        WriteStringToFileService::new(sys.clone()),
    );
    router.add_service(
        join(prefix, r::WRITE_BYTES_TO_FILE),
        WriteBytesToFileService::new(sys.clone()),
    );
    router.add_service(
        join(prefix, r::PATH_EXISTS),
        PathExistsService::new(sys.clone()),
    );
    router.add_service(
        join(prefix, r::CREATE_DIRECTORY),
        CreateDirectoryService::new(sys.clone()),
    );
    router.add_service(
        join(prefix, r::READ_DIRECTORY),
        ReadDirectoryService::new(sys.clone()),
    );
    router
        .add_service(join(prefix, r::REMOVE), RemoveService::new(sys.clone()));
    router
        .add_service(join(prefix, r::RENAME), RenameService::new(sys.clone()));
    router.add_service(join(prefix, r::STAT), StatService::new(sys.clone()));
    router
        .add_service(join(prefix, r::IS_FILE), IsFileService::new(sys.clone()));
    router.add_service(
        join(prefix, r::IS_DIRECTORY),
        IsDirectoryService::new(sys.clone()),
    );
    router.add_service(
        join(prefix, r::IS_SYMBOLIC_LINK),
        IsSymbolicLinkService::new(sys.clone()),
    );
    router.add_service(join(prefix, r::COPY), CopyService::new(sys.clone()));
    router.add_service(
        join(prefix, r::APPEND_STRING_TO_FILE),
        AppendStringToFileService::new(sys),
    );
}

/// Registers every process service against the given router under
/// `prefix`.
pub fn register_proc_services<S, P>(
    router: &mut Router,
    sys: Arc<S>,
    args_provider: Arc<P>,
    prefix: &str,
) where
    S: ProcSys,
    P: ArgsProvider,
{
    use proc_routes as r;

    router.add_service(
        join(prefix, r::CURRENT_DIR),
        CurrentDirService::new(sys.clone()),
    );
    router.add_service(
        join(prefix, r::SET_CURRENT_DIR),
        SetCurrentDirService::new(sys.clone()),
    );
    router.add_service(
        join(prefix, r::ARGS),
        ArgsService::new(args_provider.clone()),
    );
    router.add_service(join(prefix, r::ENV), EnvService::new(sys.clone()));
    router.add_service(
        join(prefix, r::SNAPSHOT),
        SnapshotService::new(sys, args_provider),
    );
}

/// Registers the log service against the given router at `path`.
pub fn register_log_service<L>(router: &mut Router, logger: L, path: &str)
where
    L: Log + 'static,
{
    router.add_service(path, LogService::new(logger));
}

/// Registers every service exported by this crate against `router`.
///
/// This is the one-stop wiring helper. Each underlying registration step
/// is also exposed individually
/// ([`register_fs_services`], [`register_proc_services`],
/// [`register_log_service`]) for callers that want finer-grained control.
///
/// Type parameters
/// ---------------
/// - `S`: the system handle used by both the FS services (must satisfy
///   [`FsSys`]) and the process services (must satisfy [`ProcSys`]).
/// - `P`: the [`ArgsProvider`] used by `ArgsService` and `SnapshotService`.
///   Defaults to [`StdArgsProvider`] via [`register_services_with_defaults`].
/// - `L`: the [`log::Log`] implementation used by `LogService`.
pub fn register_services<S, P, L>(
    router: &mut Router,
    sys: Arc<S>,
    args_provider: Arc<P>,
    logger: L,
    options: RegisterServicesOptions,
) where
    S: FsSys + ProcSys,
    <S as BaseFsMetadataAsync>::Metadata: Send,
    P: ArgsProvider,
    L: Log + 'static,
{
    register_fs_services(router, sys.clone(), &options.fs_prefix);
    register_proc_services(router, sys, args_provider, &options.proc_prefix);
    register_log_service(router, logger, &options.log_path);
}

/// Convenience variant of [`register_services`] that uses
/// [`StdArgsProvider`] for process arguments and the default
/// [`log::logger`] for the log service.
///
/// Useful when the bridge service is the sole owner of these globals.
pub fn register_services_with_defaults<S>(
    router: &mut Router,
    sys: Arc<S>,
    options: RegisterServicesOptions,
) where
    S: FsSys + ProcSys,
    <S as BaseFsMetadataAsync>::Metadata: Send,
{
    register_services(
        router,
        sys,
        Arc::new(StdArgsProvider),
        log::logger(),
        options,
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bridge_rpc_core::{ResponseStatusCode, service::Service};
    use bridge_rpc_router::Router;
    use system_traits::impls::RealSys;
    use tempfile::TempDir;

    use super::*;
    use crate::services::{
        common::{RETURNS_HEADER, encode_parameters},
        fs::BoolResponse,
        proc::CurrentDirResponse,
        test_harness::ServiceContextBuilder,
    };

    #[derive(serde::Serialize)]
    struct PathParam {
        path: std::path::PathBuf,
    }

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

    /// A minimal `log::Log` impl that discards records; sufficient to
    /// verify wiring without affecting the global logger.
    #[derive(Clone, Default)]
    struct DiscardLogger;
    impl log::Log for DiscardLogger {
        fn enabled(&self, _: &log::Metadata) -> bool {
            false
        }
        fn log(&self, _: &log::Record) {}
        fn flush(&self) {}
    }

    #[tokio::test]
    async fn register_services_wires_fs_routes() {
        let mut router = Router::new();
        let sys = Arc::new(RealSys::default());

        register_services(
            &mut router,
            sys,
            Arc::new(StdArgsProvider),
            DiscardLogger,
            RegisterServicesOptions::default(),
        );

        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("a.txt");
        std::fs::write(&path, b"hi").unwrap();

        let (ctx, awaiter) = ServiceContextBuilder::new("/fs/path-exists")
            .with_headers(params_for(&PathParam { path }))
            .build()
            .await;

        router.run(ctx).await.expect("router run should succeed");

        let response = awaiter.wait().await;
        assert_eq!(response.status, ResponseStatusCode::SUCCESS);
        let parsed: BoolResponse = read_response_returns(&response.headers);
        assert!(parsed.value);
    }

    #[tokio::test]
    async fn register_services_wires_proc_routes() {
        let mut router = Router::new();
        let sys = Arc::new(RealSys::default());

        register_services(
            &mut router,
            sys,
            Arc::new(StdArgsProvider),
            DiscardLogger,
            RegisterServicesOptions::default(),
        );

        let (ctx, awaiter) = ServiceContextBuilder::new("/proc/current-dir")
            .build()
            .await;
        router.run(ctx).await.expect("router run should succeed");

        let response = awaiter.wait().await;
        assert_eq!(response.status, ResponseStatusCode::SUCCESS);
        let parsed: CurrentDirResponse =
            read_response_returns(&response.headers);
        assert!(!parsed.current_dir.is_empty());
    }

    #[tokio::test]
    async fn register_services_wires_log_route() {
        let mut router = Router::new();
        let sys = Arc::new(RealSys::default());

        register_services(
            &mut router,
            sys,
            Arc::new(StdArgsProvider),
            DiscardLogger,
            RegisterServicesOptions::default(),
        );

        // Hitting `/log` with a valid payload should succeed.
        let body = serde_json::json!({
            "level": "info",
            "target": ["t"],
            "message": "m",
            "timestamp": 1u64,
        });
        let (ctx, mut awaiter) = ServiceContextBuilder::new("/log")
            .with_body_json(&body)
            .build()
            .await;
        router.run(ctx).await.expect("router run should succeed");

        // The log service does not produce any frames; it just consumes
        // the request and returns. Verify the channel is drained.
        assert!(awaiter.is_drained());
    }

    #[tokio::test]
    async fn unknown_path_returns_no_handler() {
        let mut router = Router::new();
        let sys = Arc::new(RealSys::default());

        register_services(
            &mut router,
            sys,
            Arc::new(StdArgsProvider),
            DiscardLogger,
            RegisterServicesOptions::default(),
        );

        let (ctx, awaiter) =
            ServiceContextBuilder::new("/no-such-route").build().await;
        router.run(ctx).await.expect("router run should succeed");

        let response = awaiter.wait().await;
        assert_eq!(response.status, ResponseStatusCode::NO_HANDLER_FOR_PATH);
    }

    #[tokio::test]
    async fn custom_prefixes_are_honoured() {
        let mut router = Router::new();
        let sys = Arc::new(RealSys::default());

        register_services(
            &mut router,
            sys,
            Arc::new(StdArgsProvider),
            DiscardLogger,
            RegisterServicesOptions::new()
                .with_fs_prefix("/api/fs")
                .with_proc_prefix("/api/proc")
                .with_log_path("/api/log"),
        );

        // Default `/proc/current-dir` must NOT be registered.
        let (ctx, awaiter) = ServiceContextBuilder::new("/proc/current-dir")
            .build()
            .await;
        router.run(ctx).await.expect("router run should succeed");
        let response = awaiter.wait().await;
        assert_eq!(response.status, ResponseStatusCode::NO_HANDLER_FOR_PATH);

        // The custom-prefixed path should work instead.
        let (ctx, awaiter) =
            ServiceContextBuilder::new("/api/proc/current-dir")
                .build()
                .await;
        router.run(ctx).await.expect("router run should succeed");
        let response = awaiter.wait().await;
        assert_eq!(response.status, ResponseStatusCode::SUCCESS);
    }
}
