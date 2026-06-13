//! Lazily-initialized JavaScript generator script runner(s).
//!
//! A generator run may execute any number of `run-javascript` actions, possibly
//! nested through `run-generator` actions. To avoid spawning a JS process per
//! script, the whole run shares a single [`LazyScriptRunner`]:
//!
//! * It is created once, at the top of [`run_in_transaction`](crate::run), and
//!   threaded (by reference) through every nested generator invocation.
//! * A JS process is spawned **lazily**, on the first `run-javascript` action
//!   that needs a given runtime. Subsequent calls (including those from nested
//!   generators) reuse it.
//! * Because each `run-javascript` action may request a different runtime
//!   (`node`/`bun`/`deno`/`auto`), the runner keeps **one process per resolved
//!   runtime**. `auto` resolves to a single detected runtime, so the common
//!   "every action uses the default" case still spawns exactly one process.
//! * Its file-system / process / log services are backed by the same
//!   [`TransactionSys`] overlay used by the rest of the generator, so JS side
//!   effects participate in the same transaction (and honour dry runs).

use std::{
    collections::HashMap, future::Future, path::PathBuf, pin::Pin, sync::Arc,
};

use bridge_rpc_router::Router;
use bridge_rpc_services::{
    RegisterServicesOptions, register_services_with_defaults,
};
use js_runtime::{
    BridgeServiceRunner, VendoredBridgeService,
    impls::DelegatingJsRuntimeOption,
};
use serde::Serialize;
use tokio::sync::Mutex;

use crate::{GeneratorSys, TransactionSys, error::Error};

/// Path of the `exec-generator-script` service exposed by the bridge service.
const EXEC_GENERATOR_SCRIPT_PATH: &str = "/exec-generator-script";

type RunnerFuture =
    Pin<Box<dyn Future<Output = Result<BridgeServiceRunner, Error>> + Send>>;
/// Spawns a runner for a concrete (already-resolved) runtime.
type RunnerFactory =
    Box<dyn Fn(DelegatingJsRuntimeOption) -> RunnerFuture + Send + Sync>;

/// Parameters handed to a single generator script invocation.
#[derive(Debug, Clone, Serialize)]
pub struct ScriptParams {
    /// Whether the current generator run is a dry run.
    pub dry_run: bool,
    /// Arbitrary, already-templated data provided by the action configuration.
    pub data: serde_json::Value,
}

/// A single `{ path, params }` entry in the `exec-generator-script` payload.
#[derive(Debug, Clone, Serialize)]
pub struct ScriptInvocation {
    /// Absolute path of the script to execute.
    pub path: String,
    /// Per-script parameters.
    pub params: ScriptParams,
}

/// A shared, lazily-spawned set of generator script runners keyed by runtime.
pub struct LazyScriptRunner {
    /// One spawned process per *resolved* runtime.
    runners:
        Mutex<HashMap<DelegatingJsRuntimeOption, Arc<BridgeServiceRunner>>>,
    factory: RunnerFactory,
}

impl std::fmt::Debug for LazyScriptRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LazyScriptRunner").finish_non_exhaustive()
    }
}

impl LazyScriptRunner {
    /// Creates a runner handle whose backing JS process(es) are spawned on first
    /// use.
    ///
    /// * `sys` is the transactional overlay whose file-system / process / log
    ///   operations are exposed to the JS scripts.
    /// * `context_dir` is where the vendored bundle is materialized and the JS
    ///   process is launched (typically the workspace directory).
    /// * `version` is baked into the vendored bundle so the binary always runs
    ///   the bundle it shipped with.
    pub fn new<S>(
        sys: TransactionSys<S>,
        context_dir: PathBuf,
        version: String,
    ) -> Self
    where
        S: GeneratorSys,
    {
        let factory: RunnerFactory = Box::new(move |runtime| {
            let sys = sys.clone();
            let context_dir = context_dir.clone();
            let version = version.clone();

            Box::pin(async move {
                let vendored = VendoredBridgeService::new(version)
                    .ensure(&context_dir)
                    .await
                    .map_err(|e| Error::custom(e.to_string()))?;

                let mut router = Router::new();
                register_services_with_defaults(
                    &mut router,
                    Arc::new(sys),
                    RegisterServicesOptions::default(),
                );

                BridgeServiceRunner::spawn(
                    &vendored.entrypoint,
                    runtime,
                    router,
                    Some(&context_dir),
                )
                .await
                .map_err(|e| Error::custom(e.to_string()))
            })
        });

        Self {
            runners: Mutex::new(HashMap::new()),
            factory,
        }
    }

    /// Runs the generator scripts described by `invocations` on the shared
    /// process for `runtime`, spawning that process if it isn't running yet.
    pub async fn run_scripts(
        &self,
        runtime: DelegatingJsRuntimeOption,
        invocations: &[ScriptInvocation],
    ) -> Result<(), Error> {
        // Resolve `Auto` to a concrete runtime so every `Auto` request shares a
        // single process (and so distinct explicit runtimes get distinct keys).
        let resolved = runtime.resolve().ok_or_else(|| {
            Error::custom("no JS runtime (node/bun/deno) found on PATH")
        })?;

        let runner = self.runner_for(resolved).await?;

        runner
            .call(EXEC_GENERATOR_SCRIPT_PATH, invocations)
            .await
            .map(|_| ())
            .map_err(|e| Error::custom(e.to_string()))
    }

    /// Returns the (possibly newly-spawned) runner for a concrete `runtime`.
    ///
    /// Actions run sequentially within a generation, so a single mutex around
    /// the process table is sufficient and avoids spawning duplicates.
    async fn runner_for(
        &self,
        runtime: DelegatingJsRuntimeOption,
    ) -> Result<Arc<BridgeServiceRunner>, Error> {
        let mut runners = self.runners.lock().await;
        if let Some(runner) = runners.get(&runtime) {
            return Ok(runner.clone());
        }

        let runner = Arc::new((self.factory)(runtime).await?);
        runners.insert(runtime, runner.clone());
        Ok(runner)
    }

    /// Shuts down every runner that was started. Best-effort.
    pub async fn shutdown(&self) {
        let runners = {
            let mut guard = self.runners.lock().await;
            std::mem::take(&mut *guard)
        };
        for (_, runner) in runners {
            let _ = runner.shutdown().await;
        }
    }
}
