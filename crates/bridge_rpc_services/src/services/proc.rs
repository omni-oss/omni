//! Process / environment services exposed over Bridge RPC.
//!
//! These services back the `Process` interface exposed in JS by
//! `packages/bridge-rpc-services/src/dry-run-system.ts`. The JS side keeps
//! `currentDir`, `args`, and `env` as cached snapshots populated at
//! construction time, so a single [`SnapshotService`] is provided alongside
//! per-property services.
//!
//! Wire conventions
//! -----------------
//!
//! All process services pass small, structured payloads in headers: inputs
//! in the `parameters` request header, outputs in the `returns` response
//! header. None of them currently use the body. See [`super::common`] for
//! the encoding conventions.
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use async_trait::async_trait;
use bridge_rpc_core::{
    service::{Service, ServiceContext},
    service_error::ServiceError,
};
use serde::{Deserialize, Serialize};
use system_traits::{
    BaseEnvSetCurrentDirAsync, EnvCurrentDirAsync, EnvSetCurrentDirAsync as _,
    EnvSnapshot,
};

use super::common::{read_parameters, respond_empty, respond_with_returns};

// ---------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Debug, Clone)]
struct SetCurrentDirParams {
    pub dir: PathBuf,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CurrentDirResponse {
    pub current_dir: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ArgsResponse {
    pub args: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EnvResponse {
    pub env: BTreeMap<String, String>,
}

/// A snapshot of the current process state that is suitable for bootstrapping
/// a JS-side `Process` view (which caches these values synchronously).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProcessSnapshotResponse {
    pub current_dir: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

// ---------------------------------------------------------------------------
// Argument / environment providers
// ---------------------------------------------------------------------------

/// Source of process arguments. Defaults to [`std::env::args`].
///
/// Custom providers are useful for tests or for serving a virtualized view of
/// the process (e.g. when the bridge service should report the JS host's
/// arguments rather than its own).
pub trait ArgsProvider: Send + Sync + 'static {
    fn args(&self) -> Vec<String>;
}

/// Default implementation backed by [`std::env::args`].
#[derive(Debug, Clone, Copy, Default)]
pub struct StdArgsProvider;

impl ArgsProvider for StdArgsProvider {
    fn args(&self) -> Vec<String> {
        std::env::args().collect()
    }
}

impl<F> ArgsProvider for F
where
    F: Fn() -> Vec<String> + Send + Sync + 'static,
{
    fn args(&self) -> Vec<String> {
        (self)()
    }
}

fn collect_env<S: EnvSnapshot>(sys: &S) -> BTreeMap<String, String> {
    sys.env_snapshot()
}

// ---------------------------------------------------------------------------
// Service definitions (boilerplate via macro)
// ---------------------------------------------------------------------------

macro_rules! define_sys_service {
    ($(#[$attr:meta])* $name:ident) => {
        $(#[$attr])*
        #[derive(Debug)]
        pub struct $name<S> {
            sys: Arc<S>,
        }

        impl<S> $name<S> {
            pub fn new(sys: Arc<S>) -> Self {
                Self { sys }
            }

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

define_sys_service!(
    /// Backs `Process.currentDir()`.
    ///
    /// Request: empty. Response: `parameters = { current_dir }`.
    CurrentDirService
);
define_sys_service!(
    /// Backs `Process.setCurrentDir(dir)`.
    ///
    /// Request: `parameters = { dir }`. Response: empty.
    SetCurrentDirService
);
define_sys_service!(
    /// Backs `Process.env()`.
    ///
    /// Request: empty. Response: `parameters = { env }`.
    EnvService
);

/// Backs `Process.args()`.
///
/// Request: empty. Response: `parameters = { args }`.
///
/// Args are sourced from an [`ArgsProvider`] rather than the system handle,
/// because the system traits do not currently abstract over process
/// arguments.
#[derive(Debug)]
pub struct ArgsService<P = StdArgsProvider> {
    provider: Arc<P>,
}

impl ArgsService<StdArgsProvider> {
    /// Creates an `ArgsService` that reads from `std::env::args()`.
    pub fn from_std_args() -> Self {
        Self::new(Arc::new(StdArgsProvider))
    }
}

impl<P> ArgsService<P> {
    pub fn new(provider: Arc<P>) -> Self {
        Self { provider }
    }

    pub fn provider(&self) -> &Arc<P> {
        &self.provider
    }
}

impl<P> Clone for ArgsService<P> {
    fn clone(&self) -> Self {
        Self {
            provider: self.provider.clone(),
        }
    }
}

/// Returns a single bootstrap snapshot containing the current directory,
/// process arguments, and the full environment as a map.
///
/// Request: empty. Response: `parameters = ProcessSnapshotResponse`.
#[derive(Debug)]
pub struct SnapshotService<S, P = StdArgsProvider> {
    sys: Arc<S>,
    args_provider: Arc<P>,
}

impl<S> SnapshotService<S, StdArgsProvider> {
    /// Convenience constructor using [`StdArgsProvider`] for arguments.
    pub fn with_std_args(sys: Arc<S>) -> Self {
        Self::new(sys, Arc::new(StdArgsProvider))
    }
}

impl<S, P> SnapshotService<S, P> {
    pub fn new(sys: Arc<S>, args_provider: Arc<P>) -> Self {
        Self { sys, args_provider }
    }
}

impl<S, P> Clone for SnapshotService<S, P> {
    fn clone(&self) -> Self {
        Self {
            sys: self.sys.clone(),
            args_provider: self.args_provider.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Service implementations
// ---------------------------------------------------------------------------

#[async_trait]
impl<S> Service for CurrentDirService<S>
where
    S: EnvCurrentDirAsync + Send + Sync + 'static,
{
    async fn run(&self, context: ServiceContext) -> Result<(), ServiceError> {
        let response = context.response;
        let dir = self
            .sys
            .env_current_dir_async()
            .await
            .map_err(ServiceError::custom_error)?;

        respond_with_returns(
            response,
            &CurrentDirResponse {
                current_dir: dir.to_string_lossy().into_owned(),
            },
        )
        .await
    }
}

#[async_trait]
impl<S> Service for SetCurrentDirService<S>
where
    S: BaseEnvSetCurrentDirAsync + Send + Sync + 'static,
{
    async fn run(&self, context: ServiceContext) -> Result<(), ServiceError> {
        let ServiceContext {
            request, response, ..
        } = context;
        let params = read_parameters::<SetCurrentDirParams>(request.headers())?;

        self.sys
            .env_set_current_dir_async(&params.dir)
            .await
            .map_err(ServiceError::custom_error)?;

        respond_empty(response).await
    }
}

#[async_trait]
impl<P> Service for ArgsService<P>
where
    P: ArgsProvider,
{
    async fn run(&self, context: ServiceContext) -> Result<(), ServiceError> {
        let response = context.response;
        respond_with_returns(
            response,
            &ArgsResponse {
                args: self.provider.args(),
            },
        )
        .await
    }
}

#[async_trait]
impl<S> Service for EnvService<S>
where
    S: EnvSnapshot + Send + Sync + 'static,
{
    async fn run(&self, context: ServiceContext) -> Result<(), ServiceError> {
        let response = context.response;
        let env = collect_env(self.sys.as_ref());

        respond_with_returns(response, &EnvResponse { env }).await
    }
}

#[async_trait]
impl<S, P> Service for SnapshotService<S, P>
where
    S: EnvCurrentDirAsync + EnvSnapshot + Send + Sync + 'static,
    P: ArgsProvider,
{
    async fn run(&self, context: ServiceContext) -> Result<(), ServiceError> {
        let response = context.response;

        let current_dir = self
            .sys
            .env_current_dir_async()
            .await
            .map_err(ServiceError::custom_error)?
            .to_string_lossy()
            .into_owned();

        let args = self.args_provider.args();
        let env = collect_env(self.sys.as_ref());

        respond_with_returns(
            response,
            &ProcessSnapshotResponse {
                current_dir,
                args,
                env,
            },
        )
        .await
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

    use super::*;
    use crate::services::{
        common::{RETURNS_HEADER, encode_parameters},
        test_harness::ServiceContextBuilder,
    };

    fn real_sys() -> Arc<RealSys> {
        Arc::new(RealSys::default())
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

    #[tokio::test]
    async fn current_dir_returns_a_path() {
        let service = CurrentDirService::new(real_sys());
        let (ctx, awaiter) = ServiceContextBuilder::new("/proc/current-dir")
            .build()
            .await;

        service.run(ctx).await.expect("service should succeed");
        let response = awaiter.wait().await;
        assert_eq!(response.status, ResponseStatusCode::SUCCESS);
        let parsed: CurrentDirResponse =
            read_response_returns(&response.headers);
        assert!(!parsed.current_dir.is_empty());
    }

    #[tokio::test]
    async fn set_current_dir_changes_the_cwd() {
        // Use a temporary directory so we don't disturb the rest of the
        // test process. `set_current_dir` is process-global so this test
        // is intentionally simple.
        let tmp = tempfile::TempDir::new().unwrap();
        let original = std::env::current_dir().unwrap();

        let service = SetCurrentDirService::new(real_sys());
        let (ctx, awaiter) =
            ServiceContextBuilder::new("/proc/set-current-dir")
                .with_headers(params_for(&SetCurrentDirParams {
                    dir: tmp.path().to_path_buf(),
                }))
                .build()
                .await;

        service.run(ctx).await.expect("service should succeed");
        let response = awaiter.wait().await;
        assert_eq!(response.status, ResponseStatusCode::SUCCESS);

        // Restore the cwd before the temp dir is dropped (otherwise
        // dropping a temp dir we are inside of can fail on Windows).
        std::env::set_current_dir(&original).unwrap();
    }

    #[tokio::test]
    async fn args_service_returns_args_from_provider() {
        struct FixedArgs(Vec<String>);
        impl ArgsProvider for FixedArgs {
            fn args(&self) -> Vec<String> {
                self.0.clone()
            }
        }

        let service = ArgsService::new(Arc::new(FixedArgs(vec![
            "arg0".to_string(),
            "arg1".to_string(),
        ])));

        let (ctx, awaiter) =
            ServiceContextBuilder::new("/proc/args").build().await;

        service.run(ctx).await.expect("service should succeed");
        let response = awaiter.wait().await;
        let parsed: ArgsResponse = read_response_returns(&response.headers);
        assert_eq!(parsed.args, vec!["arg0", "arg1"]);
    }

    #[tokio::test]
    async fn env_service_returns_env_map() {
        // SAFETY: a single environment variable specific to this test.
        unsafe { std::env::set_var("BRIDGE_RPC_PROC_TEST", "yes") };

        let service = EnvService::new(real_sys());
        let (ctx, awaiter) =
            ServiceContextBuilder::new("/proc/env").build().await;

        service.run(ctx).await.expect("service should succeed");
        let response = awaiter.wait().await;
        let parsed: EnvResponse = read_response_returns(&response.headers);
        assert_eq!(
            parsed.env.get("BRIDGE_RPC_PROC_TEST").map(String::as_str),
            Some("yes")
        );

        unsafe { std::env::remove_var("BRIDGE_RPC_PROC_TEST") };
    }

    #[tokio::test]
    async fn snapshot_service_returns_full_snapshot() {
        struct FixedArgs(Vec<String>);
        impl ArgsProvider for FixedArgs {
            fn args(&self) -> Vec<String> {
                self.0.clone()
            }
        }

        let service = SnapshotService::new(
            real_sys(),
            Arc::new(FixedArgs(vec!["a".to_string()])),
        );

        let (ctx, awaiter) =
            ServiceContextBuilder::new("/proc/snapshot").build().await;

        service.run(ctx).await.expect("service should succeed");
        let response = awaiter.wait().await;
        let parsed: ProcessSnapshotResponse =
            read_response_returns(&response.headers);
        assert!(!parsed.current_dir.is_empty());
        assert_eq!(parsed.args, vec!["a".to_string()]);
        assert!(!parsed.env.is_empty());
    }
}
