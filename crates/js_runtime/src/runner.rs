//! A long-lived JavaScript "bridge service" runner.
//!
//! [`BridgeServiceRunner`] spawns a JavaScript process and connects to it with
//! a bidirectional [`BridgeRpc`] over the child's stdio:
//!
//! * **Outgoing** – [`BridgeServiceRunner::call`] issues a request to a service
//!   path on the JS side, passing arbitrary serializable data as the body.
//! * **Incoming** – the JS process can call back into the [`Service`] supplied
//!   at spawn time (typically a [`Router`] wired with the `bridge_rpc_services`
//!   file-system / process / log services).
//!
//! The runner is intentionally generic: it knows nothing about *which* JS
//! service it talks to, only how to spawn the process, keep the RPC alive, and
//! forward requests.

use std::{path::Path, process::Stdio, time::Duration};

use bridge_rpc_core::{
    BridgeRpc, ResponseStatusCode, StreamTransport,
    client::{request::PendingRequest, response::Response},
    service::Service,
};
use bridge_rpc_router::Router;
use serde::Serialize;
use tokio::{
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::Mutex,
    task::JoinHandle,
};

use crate::{JsRuntimeError, error, impls::DelegatingJsRuntimeOption};

type RunnerTransport = StreamTransport<ChildStdout, ChildStdin>;

/// A running JavaScript process bridged over stdio.
///
/// `TService` is the service exposed *to* the JS process (for the reverse
/// direction of the RPC). It defaults to [`Router`].
pub struct BridgeServiceRunner<TService: Service = Router> {
    rpc: BridgeRpc<RunnerTransport, TService>,
    child: Mutex<Child>,
    run_task: JoinHandle<()>,
}

impl<TService: Service> BridgeServiceRunner<TService> {
    /// Spawns the JS `entrypoint` with `runtime` and connects a [`BridgeRpc`]
    /// that serves `service` to it.
    ///
    /// `cwd`, when provided, becomes the working directory of the JS process.
    pub async fn spawn(
        entrypoint: &Path,
        runtime: DelegatingJsRuntimeOption,
        service: TService,
        cwd: Option<&Path>,
    ) -> Result<Self, JsRuntimeError> {
        let mut command = build_command(runtime, entrypoint)?;
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }

        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Leave stderr inherited so JS diagnostics surface to the user.
            .stderr(Stdio::inherit())
            .kill_on_drop(true);

        let mut child = command.spawn().map_err(|e| {
            error::error!(
                "failed to spawn bridge service ({}): {e}",
                entrypoint.display()
            )
        })?;

        let stdin = child.stdin.take().ok_or_else(|| {
            error::error!("bridge service child has no stdin handle")
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            error::error!("bridge service child has no stdout handle")
        })?;

        // We read frames from the child's stdout and write frames to its stdin.
        let transport = StreamTransport::new(stdout, stdin);
        let rpc = BridgeRpc::new(transport, service);

        let run_task = {
            let rpc = rpc.clone();
            tokio::spawn(async move {
                if let Err(e) = rpc.run().await {
                    trace::error!(error = %e, "bridge service rpc loop ended");
                }
            })
        };

        Ok(Self {
            rpc,
            child: Mutex::new(child),
            run_task,
        })
    }

    /// Issues a request to `path` on the JS side, sending `data` (serialized as
    /// JSON) as the request body, and returns the response body bytes.
    ///
    /// Spinning up the RPC event loop is awaited transparently. A non-success
    /// response status is turned into an error carrying the response body.
    pub async fn call<T>(
        &self,
        path: &str,
        data: &T,
    ) -> Result<Vec<u8>, JsRuntimeError>
    where
        T: Serialize + ?Sized,
    {
        let body = serde_json::to_vec(data).map_err(|e| {
            error::error!("failed to serialize request body: {e}")
        })?;

        let pending = self.request_when_ready(path).await?;
        let mut active = pending.start().await.map_err(|e| {
            error::error!("failed to start `{path}` request: {e}")
        })?;
        active.write_body_chunk(body).await.map_err(|e| {
            error::error!("failed to send `{path}` request body: {e}")
        })?;
        let pending_response = active.end().await.map_err(|e| {
            error::error!("failed to finish `{path}` request: {e}")
        })?;
        let response = pending_response.wait().await.map_err(|e| {
            error::error!("failed to receive `{path}` response: {e}")
        })?;

        let status = response.status();
        if status == ResponseStatusCode::SUCCESS {
            return Ok(read_body_bytes(response).await);
        }

        let message = String::from_utf8_lossy(&read_body_bytes(response).await)
            .into_owned();
        Err(error::error!(
            "`{path}` failed (status {}): {message}",
            status.code()
        )
        .into())
    }

    /// Shuts the runner down: closes the RPC and terminates the child process.
    pub async fn shutdown(&self) -> Result<(), JsRuntimeError> {
        let _ = self.rpc.close().await;
        self.run_task.abort();
        let mut child = self.child.lock().await;
        let _ = child.start_kill();
        let _ = child.wait().await;
        Ok(())
    }

    /// Issues the request, retrying briefly while the RPC event loop spins up.
    async fn request_when_ready(
        &self,
        path: &str,
    ) -> Result<PendingRequest, JsRuntimeError> {
        const MAX_ATTEMPTS: usize = 50;
        let mut last_err = None;
        for _ in 0..MAX_ATTEMPTS {
            match self.rpc.request(path).await {
                Ok(pending) => return Ok(pending),
                Err(e) => {
                    last_err = Some(e);
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
        }
        Err(error::error!(
            "bridge service did not become ready: {}",
            last_err
                .map(|e| e.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        )
        .into())
    }
}

/// Reads the full response body into a byte buffer.
async fn read_body_bytes(response: Response) -> Vec<u8> {
    let mut reader = response.into_reader();
    let mut buf = Vec::new();
    loop {
        match reader.read_body_chunk().await {
            Ok(Some(chunk)) => buf.extend_from_slice(&chunk),
            Ok(None) => break,
            Err(_) => break,
        }
    }
    buf
}

/// Builds the spawn command for the configured runtime, resolving `Auto`.
fn build_command(
    runtime: DelegatingJsRuntimeOption,
    entrypoint: &Path,
) -> Result<Command, JsRuntimeError> {
    let runtime = runtime.resolve().ok_or_else(|| {
        error::error!("no JS runtime (node/bun/deno) found on PATH")
    })?;

    let mut command = match runtime {
        DelegatingJsRuntimeOption::Node => Command::new("node"),
        DelegatingJsRuntimeOption::Bun => {
            let mut c = Command::new("bun");
            c.arg("run");
            c
        }
        DelegatingJsRuntimeOption::Deno => {
            let mut c = Command::new("deno");
            c.arg("run").arg("--allow-all");
            c
        }
        DelegatingJsRuntimeOption::Auto => {
            unreachable!("Auto runtime resolved above")
        }
    };

    // The entrypoint file followed by the `run` subcommand the CLI expects.
    command.arg(entrypoint).arg("run");
    Ok(command)
}
