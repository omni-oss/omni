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
//! The runner is intentionally generic and knows nothing about *which* JS
//! service it talks to: callers describe the launch via [`BridgeRunnerOptions`]
//! (entrypoint, runtime, working directory, capability [`SpawnPolicy`], and any
//! script-specific trailing arguments), and the runner spawns the process,
//! keeps the RPC alive, and forwards requests. This lets any subsystem
//! (generators today, tools later) reuse the same bridge machinery.
//!
//! ## Confinement
//!
//! The runtime is launched under a [`SpawnPolicy`] — the capability-derived set
//! of launch restrictions produced by `omni_capability_enforcement`. Nothing in
//! this crate grants a blanket `--allow-all`; a runtime handed an empty policy
//! runs with whatever confinement that runtime defaults to (for Deno, fully
//! locked down). Building an appropriate policy is the caller's responsibility.

use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};

use bridge_rpc_core::{
    BridgeRpc, ResponseStatusCode, StreamTransport,
    client::{request::PendingRequest, response::Response},
    service::Service,
};
use bridge_rpc_router::Router;
use omni_capability_enforcement::{OsSandboxSpec, SpawnPolicy};
use serde::Serialize;
use tokio::{
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::{Mutex, watch},
    task::JoinHandle,
};

use crate::{BridgeRunnerError, DelegatingJsRuntimeOption, error};

type RunnerTransport = StreamTransport<ChildStdout, ChildStdin>;

/// How to launch a bridge service process.
///
/// Everything that varies between call sites lives here so
/// [`BridgeServiceRunner::spawn`] stays a single, stable entry point that other
/// subsystems can reuse.
#[derive(Debug, Clone, Copy)]
pub struct BridgeRunnerOptions<'a> {
    /// The JavaScript entrypoint (module) to execute.
    pub entrypoint: &'a Path,
    /// Which runtime to launch (`Auto` is resolved against `PATH`).
    pub runtime: DelegatingJsRuntimeOption,
    /// Working directory for the child process, when set.
    pub cwd: Option<&'a Path>,
    /// Capability-derived launch restrictions, replacing any blanket
    /// `--allow-all`. Pass an empty policy for a fully-defaulted (locked-down,
    /// on Deno) launch.
    pub spawn_policy: &'a SpawnPolicy,
    /// Arguments passed to the script *after* the entrypoint, e.g. a subcommand
    /// the CLI expects (`["run"]` for the bridge-service CLI). Empty for a bare
    /// module.
    pub script_args: &'a [&'a str],
}

impl<'a> BridgeRunnerOptions<'a> {
    /// Convenience constructor for the common case: an entrypoint, a runtime,
    /// and a policy, with no `cwd` and no trailing script arguments.
    pub fn new(
        entrypoint: &'a Path,
        runtime: DelegatingJsRuntimeOption,
        spawn_policy: &'a SpawnPolicy,
    ) -> Self {
        Self {
            entrypoint,
            runtime,
            cwd: None,
            spawn_policy,
            script_args: &[],
        }
    }

    pub fn with_cwd(mut self, cwd: Option<&'a Path>) -> Self {
        self.cwd = cwd;
        self
    }

    pub fn with_script_args(mut self, script_args: &'a [&'a str]) -> Self {
        self.script_args = script_args;
        self
    }
}

/// A running JavaScript process bridged over stdio.
///
/// `TService` is the service exposed *to* the JS process (for the reverse
/// direction of the RPC). It defaults to [`Router`].
pub struct BridgeServiceRunner<TService: Service = Router> {
    rpc: BridgeRpc<RunnerTransport, TService>,
    child: Arc<Mutex<Child>>,
    run_task: JoinHandle<()>,
    /// Latest observed child exit code: `None` while the process is still
    /// running, `Some(code)` once it has exited (`code` is the process exit
    /// status, or `-1` if it was terminated by a signal). Requests race against
    /// this so a runtime that dies before/while serving an RPC (e.g. it rejected
    /// an unsupported launch flag) fails fast instead of hanging on a dead
    /// stdio pipe.
    exit_rx: watch::Receiver<Option<i32>>,
    exit_task: JoinHandle<()>,
}

impl<TService: Service> BridgeServiceRunner<TService> {
    /// Spawns the JS process described by `options` and connects a [`BridgeRpc`]
    /// that serves `service` to it.
    pub async fn spawn(
        service: TService,
        options: BridgeRunnerOptions<'_>,
    ) -> Result<Self, BridgeRunnerError> {
        let std_command = build_command(
            options.runtime,
            options.entrypoint,
            options.spawn_policy,
            options.script_args,
        )?;
        let mut command = Command::from(std_command);
        if let Some(cwd) = options.cwd {
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
                options.entrypoint.display()
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
                    trace::error!(error = %e, "bridge_service_rpc_loop_ended");
                }
            })
        };

        // Watch for the child exiting so an in-flight (or not-yet-ready) request
        // can abort promptly rather than blocking forever on a dead pipe.
        let child = Arc::new(Mutex::new(child));
        let (exit_tx, exit_rx) = watch::channel(None);
        let exit_task = {
            let child = child.clone();
            tokio::spawn(async move {
                loop {
                    {
                        let mut child = child.lock().await;
                        match child.try_wait() {
                            Ok(Some(status)) => {
                                let _ = exit_tx
                                    .send(Some(status.code().unwrap_or(-1)));
                                return;
                            }
                            Ok(None) => {}
                            // We can no longer observe the child; treat it as
                            // gone so requests do not hang.
                            Err(_) => {
                                let _ = exit_tx.send(Some(-1));
                                return;
                            }
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
            })
        };

        Ok(Self {
            rpc,
            child,
            run_task,
            exit_rx,
            exit_task,
        })
    }

    /// Issues a request to `path` on the JS side, sending `data` (serialized as
    /// JSON) as the request body, and returns the response body bytes.
    ///
    /// Spinning up the RPC event loop is awaited transparently. A non-success
    /// response status is turned into an error carrying the response body. If
    /// the runtime process exits before the request completes (for example, it
    /// rejected an unsupported launch flag), the call returns promptly with an
    /// error naming the exit code rather than hanging on the dead stdio pipe.
    pub async fn call<T>(
        &self,
        path: &str,
        data: &T,
    ) -> Result<Vec<u8>, BridgeRunnerError>
    where
        T: Serialize + ?Sized,
    {
        let body = serde_json::to_vec(data).map_err(|e| {
            error::error!("failed to serialize request body: {e}")
        })?;

        let mut exit_rx = self.exit_rx.clone();
        tokio::select! {
            result = self.call_inner(path, body) => result,
            _ = async { let _ = exit_rx.wait_for(|v| v.is_some()).await; } => {
                let code = (*exit_rx.borrow()).unwrap_or(-1);
                Err(error::error!(
                    "the JavaScript runtime exited (exit code {code}) before \
                     `{path}` completed; check the runtime's output above (a \
                     common cause is the runtime rejecting a launch flag it does \
                     not support)"
                )
                .into())
            }
        }
    }

    /// The request/response exchange itself, without the child-exit race.
    async fn call_inner(
        &self,
        path: &str,
        body: Vec<u8>,
    ) -> Result<Vec<u8>, BridgeRunnerError> {
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
    pub async fn shutdown(&self) -> Result<(), BridgeRunnerError> {
        let _ = self.rpc.close().await;
        self.run_task.abort();
        self.exit_task.abort();
        let mut child = self.child.lock().await;
        let _ = child.start_kill();
        let _ = child.wait().await;
        Ok(())
    }

    /// Issues the request, retrying briefly while the RPC event loop spins up.
    async fn request_when_ready(
        &self,
        path: &str,
    ) -> Result<PendingRequest, BridgeRunnerError> {
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

/// Builds the spawn command for the configured runtime, resolving `Auto` and
/// splicing the capability [`SpawnPolicy`] before the entrypoint.
///
/// Layout: `<runtime> [run] <policy args…> <entrypoint> <script args…>`. The
/// policy args land before the entrypoint so runtime permission flags (Deno's
/// `--allow-*` / `--deny-*`, Node's `--permission …`) apply to the executed
/// module.
///
/// Returns a [`std::process::Command`] (not a tokio one) so the caller can carry
/// over any OS-sandbox `pre_exec` hook installed here into the async command it
/// finally spawns.
fn build_command(
    runtime: DelegatingJsRuntimeOption,
    entrypoint: &Path,
    spawn_policy: &SpawnPolicy,
    script_args: &[&str],
) -> Result<std::process::Command, BridgeRunnerError> {
    let runtime = runtime.resolve().ok_or_else(|| {
        error::error!("no JS runtime (node/bun/deno) found on PATH")
    })?;

    // Preflight: if the policy lowers `net` into Node's `--allow-net` flag but the
    // installed Node predates network permissions (< the supported baseline),
    // refuse now with an actionable message rather than letting Node reject the
    // flag and die mid-handshake.
    if runtime == DelegatingJsRuntimeOption::Node
        && spawn_policy
            .args
            .iter()
            .any(|a| a.starts_with("--allow-net"))
        && !crate::runtime::node_supports_net()
    {
        return Err(error::error!(
            "this generator's `net` capability requires Node's network \
             permission flag (`--allow-net`), available from Node v{}; the \
             resolved `node` is older. Upgrade Node, or run this generator \
             with `runtime: deno` or `runtime: bun`.",
            crate::runtime::MIN_SUPPORTED_NODE_MAJOR,
        )
        .into());
    }

    let mut command = match runtime {
        DelegatingJsRuntimeOption::Node => std::process::Command::new("node"),
        DelegatingJsRuntimeOption::Bun => {
            let mut c = std::process::Command::new("bun");
            c.arg("run");
            c
        }
        DelegatingJsRuntimeOption::Deno => {
            let mut c = std::process::Command::new("deno");
            c.arg("run");
            c
        }
        DelegatingJsRuntimeOption::Auto => {
            unreachable!("Auto runtime resolved above")
        }
    };

    // Capability-derived launch restrictions (replaces the old `--allow-all`).
    command.args(&spawn_policy.args);
    command.arg(entrypoint);
    command.args(script_args);

    // Tier-3 OS sandbox (Landlock on Linux; a no-op on other targets). Applied
    // to the child via a `pre_exec` hook so it is inherited across `execve`.
    //
    // The policy's spec confines the *script's* filesystem authority, but the
    // sandbox binds the whole child — including the runtime itself — so it must
    // also be granted the paths the runtime needs merely to start and run:
    // its own executable directory and its module/compile cache. Without these
    // Landlock would deny the runtime reading its own binary or writing its
    // cache, and the spawn would fail before any script executed.
    if let Some(spec) = &spawn_policy.os_sandbox {
        let mut spec = spec.clone();
        add_runtime_essentials(runtime, &mut spec);
        omni_capability_enforcement::install_os_sandbox(&mut command, &spec);
    }

    Ok(command)
}

/// Grant the resolved runtime the filesystem access it needs to *start and run*
/// under an OS sandbox: its own executable directory (to `execve` and load its
/// shared libraries) plus the writable module/compile cache it maintains.
///
/// System library prefixes (`/usr`, `/lib`, `/etc`, `/proc`, …) are already in
/// the sandbox baseline; this adds only what is runtime- and
/// installation-specific and therefore cannot be baked into that baseline.
fn add_runtime_essentials(
    runtime: DelegatingJsRuntimeOption,
    spec: &mut OsSandboxSpec,
) {
    let bin = match runtime {
        DelegatingJsRuntimeOption::Node => "node",
        DelegatingJsRuntimeOption::Bun => "bun",
        DelegatingJsRuntimeOption::Deno => "deno",
        // `Auto` is resolved before this point; nothing to add otherwise.
        DelegatingJsRuntimeOption::Auto => return,
    };

    // The runtime binary's directory must be readable/executable. Follow a
    // symlink (version-manager shims are common) and grant the real target's
    // directory too.
    if let Ok(path) = which::which(bin) {
        push_parent(&mut spec.read_paths, &path);
        if let Ok(canonical) = std::fs::canonicalize(&path) {
            push_parent(&mut spec.read_paths, &canonical);
        }
    }

    // A version-manager shim (nub, nvm, fnm, volta, …) is a thin launcher that
    // re-`execve`s the *real* runtime binary living elsewhere (e.g. under
    // `~/.cache/<vm>/node/<ver>/bin`), which `which`/`canonicalize` cannot
    // reveal because the shim is not a symlink to it. Ask the runtime itself
    // where it actually runs from (`process.execPath` / `Deno.execPath()`) and
    // grant that binary's directory *and its install root* (the runtime reads
    // bundled data such as ICU alongside `bin/`), so the re-exec is permitted
    // under the sandbox.
    if let Some(real) = crate::runtime::resolved_exec_path(runtime) {
        push_parent(&mut spec.read_paths, &real);
        if let Some(bin_dir) = real.parent()
            && let Some(install_root) = bin_dir.parent()
        {
            spec.read_paths.push(install_root.to_path_buf());
        }
    }

    // Runtimes stage temporary files; grant a writable temp directory.
    let tmp = std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    ensure_writable(&mut spec.write_paths, tmp);

    // Grant read/execute for the directory of every program the policy allows
    // the script to spawn, so the confined child can `execve` it. Names in a
    // directory already covered by the sandbox baseline (e.g. `/usr/bin`) are
    // harmlessly re-added. Version-manager shims are common, so a resolved
    // symlink's real target directory is granted too.
    for program in std::mem::take(&mut spec.exec_programs) {
        if let Ok(path) = which::which(&program) {
            push_parent(&mut spec.read_paths, &path);
            if let Ok(canonical) = std::fs::canonicalize(&path) {
                push_parent(&mut spec.read_paths, &canonical);
            }
        }
    }

    match runtime {
        DelegatingJsRuntimeOption::Deno => {
            // `DENO_DIR` (module/compile cache) is written at runtime.
            let cache = std::env::var_os("DENO_DIR")
                .map(PathBuf::from)
                .or_else(|| home_dir().map(|h| h.join(".cache/deno")));
            if let Some(dir) = cache {
                ensure_writable(&mut spec.write_paths, dir);
            }
            // Global Deno config / install root (e.g. `DENO_INSTALL_ROOT`).
            if let Some(home) = home_dir() {
                spec.read_paths.push(home.join(".deno"));
            }
        }
        DelegatingJsRuntimeOption::Bun => {
            // Bun reads its runtime files and writes its module cache under its
            // install root.
            let install = std::env::var_os("BUN_INSTALL")
                .map(PathBuf::from)
                .or_else(|| home_dir().map(|h| h.join(".bun")));
            if let Some(dir) = install {
                ensure_writable(&mut spec.write_paths, dir);
            }
        }
        // Node needs no writable cache to execute a prebuilt bundle; its
        // libraries live under system prefixes already in the baseline.
        DelegatingJsRuntimeOption::Node | DelegatingJsRuntimeOption::Auto => {}
    }
}

/// Push `file`'s parent directory onto `paths`, if it has one.
fn push_parent(paths: &mut Vec<PathBuf>, file: &Path) {
    if let Some(parent) = file.parent() {
        paths.push(parent.to_path_buf());
    }
}

/// Ensure `dir` exists (Landlock grants only existing paths, and the runtime
/// may need to write into it) and grant it write access. Directory creation is
/// best-effort and runs in the *parent* process, before the sandbox is applied.
fn ensure_writable(paths: &mut Vec<PathBuf>, dir: PathBuf) {
    let _ = std::fs::create_dir_all(&dir);
    paths.push(dir);
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}
