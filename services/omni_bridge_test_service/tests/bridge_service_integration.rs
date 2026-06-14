//! Integration test that drives the TypeScript `bridge-service` from a
//! Rust host built on top of [`omni_bridge_test_service`].
//!
//! The shape of this test is intentionally similar to the
//! `omni_remote_cache_client` integration tests under
//! `crates/omni_remote_cache_client/src/default_impl.rs`:
//!
//! - We locate the built artifact for the service under test inside the
//!   workspace (the cache client tests look at
//!   `target/release/omni_remote_cache_service`; here we look at
//!   `services/bridge-service/dist/bridge-service-cli.mjs`).
//! - We use a small RAII guard ([`BridgeServiceGuard`]) to spawn the
//!   service and ensure it is torn down at the end of the test, even on
//!   panic.
//! - We exercise it with a couple of integration-style assertions: a
//!   smoke-test ping, plus a real `/exec-generator-script` round-trip
//!   that mirrors the existing TypeScript test in
//!   `services/bridge-service/src/__tests__/integration.spec.ts`.
//!
//! Prerequisites
//! -------------
//!
//! - `WORKSPACE_DIR` must be set. The omni task runner sets this
//!   automatically when invoked via `omni run test -p
//!   omni_bridge_test_service`. When running `cargo test` directly,
//!   export it manually (e.g. `export WORKSPACE_DIR=$(pwd)`).
//! - `node` must be available on `PATH` (used to launch the bridge
//!   service).
//! - `bridge-service` must be built, producing
//!   `services/bridge-service/dist/bridge-service-cli.mjs`. Done
//!   automatically when this test crate's `test` task is invoked
//!   through omni — see the project's `project.omni.yaml`, which lists
//!   `@omni-oss/bridge-service#build` as a `test` dependency.
//!
//! Each test panics with an actionable message if any of the above is
//! missing.
//!
//! Cross-language wire compatibility
//! ---------------------------------
//!
//! The Rust ([`bridge_rpc_core`]) and TypeScript
//! (`@omni-oss/bridge-rpc-core`) implementations of the bridge RPC
//! protocol agree on the on-wire encoding of every payload type:
//! `Id` is a plain msgpack `uint64`, status / error codes are plain
//! msgpack integers, and the `Frame` envelope is a `[type, data]` tuple
//! with no extra marker fields. Both ends interoperate end-to-end.
//!
//! The bridge-service can also call back to the host (e.g. on
//! `/proc/snapshot` when constructing a `BridgeRpcSystem` for dry-run
//! script execution). The [`BridgeServiceGuard`] therefore wires the
//! host with the full set of services from
//! [`bridge_rpc_services`].

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use bridge_rpc_core::ResponseStatusCode;
use bridge_rpc_router::Router;
use bridge_rpc_services::{
    RegisterServicesOptions, register_services_with_defaults,
};
use bridge_rpc_utils::client::read_response;
use ntest::timeout;
use omni_bridge_test_service::{Host, HostSpawnOptions};
use serde_json::json;
use system_traits::impls::RealSys;

// ---------------------------------------------------------------------------
// Test fixtures / discovery helpers
// ---------------------------------------------------------------------------

/// Resolve `WORKSPACE_DIR` into a path or panic with an actionable error.
fn workspace_dir() -> PathBuf {
    let raw = std::env::var("WORKSPACE_DIR").unwrap_or_else(|_| {
        panic!(
            "WORKSPACE_DIR is not set. Run via `omni run test -p \
             omni_bridge_test_service` or export WORKSPACE_DIR before \
             invoking `cargo test`."
        )
    });
    PathBuf::from(raw)
}

/// Path to the built bridge-service CLI entry point.
fn bridge_service_cli_path() -> PathBuf {
    let ws_dir = workspace_dir();
    let path = ws_dir
        .join("services")
        .join("bridge-service")
        .join("dist")
        .join("bridge-service-cli.mjs");

    if !path.exists() {
        panic!(
            "bridge-service has not been built. Expected `{}` to exist. \
             Build it with `omni run build -p @omni-oss/bridge-service` \
             (or run this test via omni so the dependency is satisfied \
             automatically).",
            path.display(),
        );
    }

    omni_utils::path::clean(path)
}

/// Path to the test fixture script that the bridge-service should
/// execute.
fn fixture_script_path() -> PathBuf {
    let path = workspace_dir()
        .join("services")
        .join("bridge-service")
        .join("src")
        .join("__tests__")
        .join("__fixtures__")
        .join("test.mjs");

    assert!(
        path.exists(),
        "fixture script not found: {}",
        path.display()
    );
    path
}

/// Build the [`HostSpawnOptions`] needed to launch the bridge-service via
/// `node`.
fn bridge_service_spawn_options(cli_path: &Path) -> HostSpawnOptions {
    HostSpawnOptions::new()
        .with_child_binary(PathBuf::from("node"))
        .with_child_args([
            cli_path.to_string_lossy().into_owned(),
            "run".to_string(),
        ])
        // The bridge-service CLI uses its own subcommand structure
        // (`run`), so do not let omni_bridge_test_service inject `client`.
        .skip_client_subcommand(true)
        // Give the JS runtime a moment to initialize before we start
        // shooting requests at it.
        .with_warmup(Duration::from_millis(500))
}

/// Build a [`Router`] populated with the full set of [`bridge_rpc_services`]
/// against [`RealSys`]. This is what the host-side bridge exposes to the
/// child so that callbacks like `/proc/snapshot` (used by the
/// bridge-service's `BridgeRpcSystem.create` during dry-run script
/// execution) succeed.
fn bridge_service_host_router() -> Router {
    let mut router = Router::new();
    let sys = Arc::new(RealSys);
    register_services_with_defaults(
        &mut router,
        sys,
        RegisterServicesOptions::default(),
    );
    router
}

// ---------------------------------------------------------------------------
// RAII guard that spawns the service and tears it down on drop
// ---------------------------------------------------------------------------

/// Guard that owns the spawned bridge-service [`Host`]. Dropping the guard
/// kills the child process if it has not been shut down explicitly.
///
/// The shape mirrors `omni_remote_cache_client::test_utils::ChildProcessGuard`,
/// adapted to the bridge-service's stdio-based RPC protocol.
struct BridgeServiceGuard {
    host: Option<Host>,
}

impl BridgeServiceGuard {
    /// Spawn the bridge-service and return a guard owning the live host.
    async fn new() -> Self {
        let cli_path = bridge_service_cli_path();
        log::info!("cli path {cli_path:?}");
        let options = bridge_service_spawn_options(&cli_path);
        let router = bridge_service_host_router();

        let host = Host::spawn_with_router(options, router)
            .await
            .unwrap_or_else(|err| {
                panic!(
                    "failed to spawn bridge-service via node ({}): {err}. \
                     Make sure `node` is on PATH.",
                    cli_path.display(),
                )
            });

        Self { host: Some(host) }
    }

    /// Borrow the wrapped [`Host`].
    fn host(&self) -> &Host {
        self.host
            .as_ref()
            .expect("host was already taken or shut down")
    }

    /// Cleanly shut down the bridge and child process.
    async fn shutdown(mut self) {
        if let Some(host) = self.host.take()
            && let Err(err) = host.shutdown().await
        {
            log::warn!("bridge-service shutdown failed: {err}");
        }
    }
}

impl Drop for BridgeServiceGuard {
    fn drop(&mut self) {
        // If the test panics or returns early we do a best-effort kill
        // here so the child process doesn't leak. We can't `await` in
        // `Drop`, so we rely on `tokio::process::Command::kill_on_drop`
        // (set by `Host::spawn`) to actually terminate the child.
        // Forgetting the host (rather than dropping it) would leak the
        // child, so we DO drop it.
        let _ = self.host.take();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Smoke test: spawn the service, confirm the bridge is alive with a
/// ping, then shut down cleanly.
///
/// This exercises:
/// - launching `node services/bridge-service/dist/bridge-service-cli.mjs run`
///   from a Rust process,
/// - sending a `Ping` frame over the child's stdin and reading the
///   `Pong` frame off its stdout,
/// - and tearing the bridge down via a `Close` frame.
#[tokio::test(flavor = "multi_thread")]
#[test_log::test]
#[timeout(15_000)]
async fn test_bridge_service_pings_back() {
    let guard = BridgeServiceGuard::new().await;

    let pong = guard
        .host()
        .ping(Duration::from_secs(5))
        .await
        .expect("ping should succeed");

    assert!(pong, "expected ping to return true");

    guard.shutdown().await;
}

/// End-to-end test: send `/exec-generator-script` request with a
/// fixture script that just calls `ctx.log.info(...)`, and verify the
/// response status is `SUCCESS`.
#[tokio::test(flavor = "multi_thread")]
#[test_log::test]
#[timeout(15_000)]
async fn test_bridge_service_exec_generator_script() {
    let guard = BridgeServiceGuard::new().await;
    let script_path = fixture_script_path();

    let body = json!([
        {
            "path": script_path.to_string_lossy(),
            "params": {
                "dry_run": true,
                "data": null,
                "output_dir": workspace_dir(),
            },
        }
    ]);
    let body_bytes = serde_json::to_vec(&body).expect("encode body as JSON");

    let client = guard.host().client_handle();
    let pending = client
        .request("/exec-generator-script")
        .await
        .expect("create request");
    let mut active = pending.start().await.expect("start request");
    active
        .write_body_chunk(body_bytes)
        .await
        .expect("write body chunk");
    let response = active
        .end()
        .await
        .expect("end request")
        .wait()
        .await
        .expect("wait for response");

    let (status, _headers, reader) = response.into_parts();
    let (response_bytes, _trailers) =
        read_response(reader).await.expect("read response body");

    assert_eq!(
        status,
        ResponseStatusCode::SUCCESS,
        "expected `/exec-generator-script` to return SUCCESS, \
                     got status={:?}, body={}",
        status,
        String::from_utf8_lossy(&response_bytes)
    );

    guard.shutdown().await;
}

/// Sending a request to a path the bridge-service does not handle should
/// surface a `NO_HANDLER_FOR_PATH` status code, rather than time out.
///
/// This is mainly here to verify that the round-trip plumbing handles
/// non-success status codes correctly.
#[tokio::test(flavor = "multi_thread")]
#[test_log::test]
#[timeout(15_000)]
async fn test_bridge_service_unknown_path_returns_no_handler() {
    let guard = BridgeServiceGuard::new().await;

    let client = guard.host().client_handle();
    let pending = client
        .request("/this-path-does-not-exist")
        .await
        .expect("create request");
    let active = pending.start().await.expect("start request");
    let response = active
        .end()
        .await
        .expect("end request")
        .wait()
        .await
        .expect("wait for response");

    let status = response.status();
    assert_eq!(
        status,
        ResponseStatusCode::NO_HANDLER_FOR_PATH,
        "expected NO_HANDLER_FOR_PATH for an unknown route, got {status:?}",
    );

    guard.shutdown().await;
}
