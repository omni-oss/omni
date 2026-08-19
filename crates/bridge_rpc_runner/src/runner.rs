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
    collections::BTreeMap,
    future::Future,
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

/// The spawned child process, however it was launched.
///
/// Most platforms use a normal async [`tokio::process::Child`]. On **Windows**,
/// when the capability policy requires OS-level confinement, the child must be
/// created *inside* an AppContainer via a synchronous
/// [`std::process::Child`] (see
/// [`omni_capability_enforcement::appcontainer_sandbox`]) whose piped stdio is
/// adapted to async with [`ChildStdin::from_std`] / [`ChildStdout::from_std`].
/// This enum lets the runner drive either uniformly.
#[cfg_attr(target_os = "windows", allow(clippy::large_enum_variant))]
enum ChildProcess {
    /// The ordinary async child (all platforms; and Windows when unconfined).
    Async(Child),
    /// A Windows AppContainer-confined child. Launched synchronously; its stdio
    /// is bridged to async before storage here. The [`SandboxAclGuard`] revokes
    /// the filesystem grants made for this child and must outlive it, so it
    /// rides along in the variant and drops (after the child is killed) with it.
    #[cfg(target_os = "windows")]
    Confined(
        std::process::Child,
        // Held only for its `Drop`, which revokes this child's filesystem grants
        // once it has been killed/reaped — never read directly.
        #[allow(dead_code)]
        omni_capability_enforcement::appcontainer_sandbox::SandboxAclGuard,
    ),
}

impl ChildProcess {
    /// Non-blocking exit check, mirroring [`tokio::process::Child::try_wait`].
    fn try_wait(
        &mut self,
    ) -> std::io::Result<Option<std::process::ExitStatus>> {
        match self {
            ChildProcess::Async(child) => child.try_wait(),
            #[cfg(target_os = "windows")]
            ChildProcess::Confined(child, _) => child.try_wait(),
        }
    }

    /// Request termination without waiting, mirroring
    /// [`tokio::process::Child::start_kill`].
    fn start_kill(&mut self) -> std::io::Result<()> {
        match self {
            ChildProcess::Async(child) => child.start_kill(),
            #[cfg(target_os = "windows")]
            ChildProcess::Confined(child, _) => child.kill(),
        }
    }

    /// Await the child's exit. For the confined (synchronous) child this waits
    /// inline; it is only reached from [`BridgeServiceRunner::shutdown`], not the
    /// hot path.
    async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        match self {
            ChildProcess::Async(child) => child.wait().await,
            #[cfg(target_os = "windows")]
            ChildProcess::Confined(child, _) => child.wait(),
        }
    }
}

// The async child kills itself on drop (via `kill_on_drop`); the synchronous
// confined child needs an explicit kill to match that behaviour. The ACL guard
// then drops with the variant, revoking the child's grants once it is gone.
#[cfg(target_os = "windows")]
impl Drop for ChildProcess {
    fn drop(&mut self) {
        if let ChildProcess::Confined(child, _) = self {
            let _ = child.kill();
        }
    }
}

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
    /// Optional wall-clock cap on a single [`call`](BridgeServiceRunner::call).
    /// `None` (the default) races only the child's *exit*, so a hung-but-alive
    /// script would stall a call forever; set this to bound that. On expiry the
    /// child is killed and the call fails.
    pub call_timeout: Option<Duration>,
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
            call_timeout: None,
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

    /// Bound how long a single [`call`](BridgeServiceRunner::call) may run before
    /// the child is killed and the call fails. See [`Self::call_timeout`].
    pub fn with_call_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.call_timeout = timeout;
        self
    }
}

/// A running JavaScript process bridged over stdio.
///
/// `TService` is the service exposed *to* the JS process (for the reverse
/// direction of the RPC). It defaults to [`Router`].
pub struct BridgeServiceRunner<TService: Service = Router> {
    rpc: BridgeRpc<RunnerTransport, TService>,
    child: Arc<Mutex<ChildProcess>>,
    run_task: JoinHandle<()>,
    /// Latest observed child exit code: `None` while the process is still
    /// running, `Some(code)` once it has exited (`code` is the process exit
    /// status, or `-1` if it was terminated by a signal). Requests race against
    /// this so a runtime that dies before/while serving an RPC (e.g. it rejected
    /// an unsupported launch flag) fails fast instead of hanging on a dead
    /// stdio pipe.
    exit_rx: watch::Receiver<Option<i32>>,
    exit_task: JoinHandle<()>,
    /// Optional wall-clock cap applied to each [`call`](Self::call); `None`
    /// leaves calls bounded only by the child's exit. See
    /// [`BridgeRunnerOptions::call_timeout`].
    call_timeout: Option<Duration>,
    /// The confined child's per-run temp directory, removed when the runner is
    /// dropped. `None` when unconfined or off Windows. Declared last so it is
    /// dropped after `child` (whose drop kills the process), so nothing is still
    /// writing into the directory as it is removed.
    _sandbox_temp: Option<SandboxTempDir>,
    /// Whether this runner's child was launched inside an AppContainer. Only
    /// then does [`grant_read_scope`](Self::grant_read_scope) install real ACL
    /// grants; an unconfined child (and every non-Windows child) needs none.
    #[cfg(target_os = "windows")]
    confined: bool,
}

/// Owns the per-call filesystem grants admitted to a confined child, revoking
/// them when dropped. Off Windows — and on Windows when the child is unconfined
/// or the scope is empty — it carries nothing and dropping it does nothing.
#[derive(Default)]
pub struct ReadScopeGuard {
    #[cfg(target_os = "windows")]
    _acl: Option<
        omni_capability_enforcement::appcontainer_sandbox::SandboxAclGuard,
    >,
}

impl ReadScopeGuard {
    /// A guard owning no grants (unconfined, off Windows, or an empty scope).
    fn none() -> Self {
        Self::default()
    }
}

impl<TService: Service> BridgeServiceRunner<TService> {
    /// Spawns the JS process described by `options` and connects a [`BridgeRpc`]
    /// that serves `service` to it.
    pub async fn spawn(
        service: TService,
        options: BridgeRunnerOptions<'_>,
    ) -> Result<Self, BridgeRunnerError> {
        let (std_command, confinement, sandbox_temp) = build_command(
            options.runtime,
            options.entrypoint,
            options.spawn_policy,
            options.script_args,
            options.cwd,
        )?;
        // `confinement` is only consumed on Windows (AppContainer is applied at
        // spawn time there); elsewhere it is always `None`.
        #[cfg(not(target_os = "windows"))]
        let _ = &confinement;

        // Windows OS-sandbox path: launch the child *inside* an AppContainer via
        // the synchronous confined spawn, then bridge its piped stdio to async.
        #[cfg(target_os = "windows")]
        if let Some(spec) = confinement {
            let mut std_command = std_command;
            if let Some(cwd) = options.cwd {
                std_command.current_dir(strip_verbatim_prefix(cwd));
            }
            std_command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit());
            let (mut child, acl_guard) =
                omni_capability_enforcement::appcontainer_sandbox::spawn(
                    &mut std_command,
                    &spec,
                )
                .map_err(|e| {
                    error::error!(
                        "failed to spawn confined bridge service ({}): {e}",
                        options.entrypoint.display()
                    )
                })?;
            let stdin = child.stdin.take().ok_or_else(|| {
                error::error!("bridge service child has no stdin handle")
            })?;
            let stdout = child.stdout.take().ok_or_else(|| {
                error::error!("bridge service child has no stdout handle")
            })?;
            let stdin = ChildStdin::from_std(stdin).map_err(|e| {
                error::error!("failed to adopt child stdin: {e}")
            })?;
            let stdout = ChildStdout::from_std(stdout).map_err(|e| {
                error::error!("failed to adopt child stdout: {e}")
            })?;
            return Ok(Self::assemble(
                service,
                stdin,
                stdout,
                ChildProcess::Confined(child, acl_guard),
                sandbox_temp,
                options.call_timeout,
            ));
        }

        let mut command = Command::from(std_command);
        if let Some(cwd) = options.cwd {
            command.current_dir(strip_verbatim_prefix(cwd));
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

        Ok(Self::assemble(
            service,
            stdin,
            stdout,
            ChildProcess::Async(child),
            sandbox_temp,
            options.call_timeout,
        ))
    }

    /// Wire an already-spawned child (its async stdio already extracted) into a
    /// live [`BridgeRpc`] plus the RPC-loop and exit-watch tasks.
    fn assemble(
        service: TService,
        stdin: ChildStdin,
        stdout: ChildStdout,
        child: ChildProcess,
        sandbox_temp: Option<SandboxTempDir>,
        call_timeout: Option<Duration>,
    ) -> Self {
        // We read frames from the child's stdout and write frames to its stdin.
        let transport = StreamTransport::new(stdout, stdin);
        let rpc = BridgeRpc::new(transport, service);

        #[cfg(target_os = "windows")]
        let confined = matches!(child, ChildProcess::Confined(..));

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

        Self {
            rpc,
            child,
            run_task,
            exit_rx,
            exit_task,
            call_timeout,
            _sandbox_temp: sandbox_temp,
            #[cfg(target_os = "windows")]
            confined,
        }
    }

    /// Grant `read_paths` to this runner's confined child for the lifetime of
    /// the returned [`ReadScopeGuard`], then revoke them when it drops.
    ///
    /// A no-op off Windows and when the child runs unconfined (both return an
    /// empty guard). On Windows the grants are reference-counted per path
    /// through the shared AppContainer grant registry, so overlapping calls that
    /// need the same path grant it once and revoke it only when the last scope
    /// drops. The caller must hold the returned guard across the `call` that
    /// makes the child read those paths and drop it afterwards.
    pub fn grant_read_scope(&self, read_paths: &[PathBuf]) -> ReadScopeGuard {
        #[cfg(not(target_os = "windows"))]
        {
            let _ = read_paths;
            ReadScopeGuard::none()
        }
        #[cfg(target_os = "windows")]
        {
            if !self.confined || read_paths.is_empty() {
                return ReadScopeGuard::none();
            }
            match omni_capability_enforcement::appcontainer_sandbox::grant_read_scope(
                read_paths,
            ) {
                Ok(acl) => ReadScopeGuard { _acl: Some(acl) },
                Err(e) => {
                    // The child will then fail to read the un-granted paths and
                    // the `call` will surface that loudly; log so the cause is
                    // findable.
                    trace::warn!(
                        error = %e,
                        "failed to grant read scope to confined bridge child"
                    );
                    ReadScopeGuard::none()
                }
            }
        }
    }

    /// Whether this runner's child was launched inside an OS sandbox that makes
    /// [`grant_read_scope`](Self::grant_read_scope) install real ACL grants.
    ///
    /// Only a Windows AppContainer child is confined in that sense; every
    /// non-Windows child, and an unconfined Windows child (e.g. Bun, which
    /// cannot boot inside an AppContainer), reports `false`. Callers use this to
    /// skip the (unconfined) import-closure scan when no read grant would be
    /// installed anyway.
    pub fn is_confined(&self) -> bool {
        #[cfg(target_os = "windows")]
        {
            self.confined
        }
        #[cfg(not(target_os = "windows"))]
        {
            false
        }
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
        match race_call(
            self.call_inner(path, body),
            &mut exit_rx,
            self.call_timeout,
        )
        .await
        {
            CallRace::Completed(result) => result,
            CallRace::ChildExited(code) => Err(error::error!(
                "the JavaScript runtime exited (exit code {code}) before \
                 `{path}` completed; check the runtime's output above (a \
                 common cause is the runtime rejecting a launch flag it does \
                 not support)"
            )
            .into()),
            CallRace::TimedOut => {
                // A hung-but-alive script would otherwise stall this call
                // forever. Kill the child so its stdio closes and its resources
                // (and, on Windows, its sandbox grants) are released, then fail
                // with an actionable message instead of blocking the caller.
                {
                    let mut child = self.child.lock().await;
                    let _ = child.start_kill();
                }
                let secs = self
                    .call_timeout
                    .map(|d| d.as_secs_f64())
                    .unwrap_or_default();
                Err(error::error!(
                    "`{path}` did not complete within {secs}s; the JavaScript \
                     runtime was killed (a script hung or is doing more work \
                     than the configured timeout allows)"
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

// A runner may be dropped without an explicit `shutdown()` (a panic, an early
// `?`-return in the generator, or simply the pool being cleared). Do the same
// teardown `shutdown` does, synchronously and best-effort: abort the background
// tasks so they stop holding a clone of the child `Arc` (and can no longer keep
// the RPC loop alive), then request the child's termination now. Relying only on
// the child's kill-on-drop is not enough here, because the child lives behind an
// `Arc` shared with `exit_task`; killing it explicitly also ensures the process
// is signalled dead *before* `_sandbox_temp` (dropped after this) removes the
// per-run temp dir the child may still be writing into.
impl<TService: Service> Drop for BridgeServiceRunner<TService> {
    fn drop(&mut self) {
        self.run_task.abort();
        self.exit_task.abort();
        // `try_lock` (not blocking) because `drop` is sync and must not stall; if
        // a task momentarily holds the lock the child's own kill-on-drop /
        // confined-child `Drop` still terminates it once every `Arc` clone is
        // released.
        if let Ok(mut child) = self.child.try_lock() {
            let _ = child.start_kill();
        }
    }
}

/// Which of the three racing conditions ended a [`BridgeServiceRunner::call`]:
/// the request completed, the child process exited first, or the optional
/// wall-clock timeout elapsed. Extracted from `call` so the race — the part with
/// no I/O — is unit-testable without a live child.
enum CallRace<T> {
    Completed(T),
    ChildExited(i32),
    TimedOut,
}

/// Race a `call` future against the child's exit and an optional timeout.
///
/// A hung-but-alive script is the real "unusable in actual usage" risk: without
/// the timeout arm a `call` only unblocks when the request finishes *or* the
/// process dies, so a script stuck in an infinite loop stalls the caller
/// forever. `None` disables the timeout (the future never fires).
async fn race_call<F, T>(
    call: F,
    exit_rx: &mut watch::Receiver<Option<i32>>,
    timeout: Option<Duration>,
) -> CallRace<T>
where
    F: Future<Output = T>,
{
    let timeout_fut = async {
        match timeout {
            Some(d) => tokio::time::sleep(d).await,
            // No cap: never resolve, leaving `call` bounded only by exit.
            None => std::future::pending::<()>().await,
        }
    };
    tokio::select! {
        out = call => CallRace::Completed(out),
        _ = async { let _ = exit_rx.wait_for(|v| v.is_some()).await; } => {
            CallRace::ChildExited((*exit_rx.borrow()).unwrap_or(-1))
        }
        _ = timeout_fut => CallRace::TimedOut,
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

/// Strips the Windows verbatim `\\?\` prefix from a path, yielding a plain
/// `C:\…` path. A verbatim current-directory confuses some child runtimes'
/// relative-path resolution and stdio setup, so the child is launched with the
/// simplified form. No-op on non-Windows and on paths without the prefix.
fn strip_verbatim_prefix(path: &Path) -> std::path::PathBuf {
    #[cfg(target_os = "windows")]
    if let Some(stripped) = path.to_str().and_then(|s| s.strip_prefix(r"\\?\"))
    {
        return std::path::PathBuf::from(stripped);
    }
    path.to_path_buf()
}

/// The bare binary name for a concrete runtime, or `None` for `Auto` (always
/// resolved to a concrete runtime before a launch program is chosen).
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn runtime_bin(runtime: DelegatingJsRuntimeOption) -> Option<&'static str> {
    match runtime {
        DelegatingJsRuntimeOption::Node => Some("node"),
        DelegatingJsRuntimeOption::Bun => Some("bun"),
        DelegatingJsRuntimeOption::Deno => Some("deno"),
        DelegatingJsRuntimeOption::Auto => None,
    }
}

/// Which program a (possibly confined) launch should exec.
///
/// A confined launch prefers the *resolved real* runtime binary over the bare
/// `PATH` name only when the `PATH` entry is a version-manager shim, so the
/// shim's per-launch bootstrap does not run (and fork-storm) inside the
/// container. A direct binary keeps the bare name: the resolved path is the
/// same file, and a bare name avoids handing the spawn an extended-length path.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
#[derive(Debug, PartialEq, Eq)]
enum ConfinedLaunchProgram {
    /// Launch the bare runtime name (unconfined, Bun, or a direct binary).
    BareName,
    /// Launch this absolute binary — a version-manager shim's real target.
    RealBinary(PathBuf),
    /// Confinement was requested but the real binary could not be resolved;
    /// the caller warns and falls back to the bare name.
    Degraded,
}

/// Decide the launch program for a spawn.
///
/// `path_entry` is the canonicalized `PATH` entry for the runtime (from
/// `which`), `resolved` is the runtime's self-reported executable path (from
/// `process.execPath` / `Deno.execPath()`), both already canonicalized. The
/// real binary is launched only when it *differs* from the `PATH` entry (the
/// `PATH` entry is a shim); an equal path is a direct binary that keeps the
/// bare name. Any returned path has its `\\?\` verbatim prefix stripped, which
/// the process-creation APIs do not accept as a program name.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn confined_launch_program(
    confined: bool,
    path_entry: Option<&Path>,
    resolved: Option<&Path>,
) -> ConfinedLaunchProgram {
    if !confined {
        return ConfinedLaunchProgram::BareName;
    }
    match (resolved, path_entry) {
        (Some(real), Some(entry))
            if strip_verbatim_prefix(real) == strip_verbatim_prefix(entry) =>
        {
            ConfinedLaunchProgram::BareName
        }
        (Some(real), _) => {
            ConfinedLaunchProgram::RealBinary(strip_verbatim_prefix(real))
        }
        (None, _) => ConfinedLaunchProgram::Degraded,
    }
}

/// Builds the spawn command for the configured runtime, resolving `Auto` and
/// splicing the capability [`SpawnPolicy`] before the entrypoint.
///
/// Layout: `<runtime> [run] <policy args…> <entrypoint> <script args…>`. The
/// policy args land before the entrypoint so runtime permission flags (Deno's
/// `--allow-*` / `--deny-*`, Node's `--permission …`) apply to the executed
/// module.
///
/// Returns a [`std::process::Command`] (not a tokio one), plus the OS-sandbox
/// spec to confine the launch with **at spawn time on Windows** (AppContainer
/// cannot be installed onto a `Command` for a later `spawn`). On non-Windows the
/// second element is always `None`: confinement there is installed onto the
/// command in place (Landlock's `pre_exec` hook) and carried over into the async
/// command the caller finally spawns.
fn build_command(
    runtime: DelegatingJsRuntimeOption,
    entrypoint: &Path,
    spawn_policy: &SpawnPolicy,
    script_args: &[&str],
    cwd: Option<&Path>,
) -> Result<
    (
        std::process::Command,
        Option<OsSandboxSpec>,
        Option<SandboxTempDir>,
    ),
    BridgeRunnerError,
> {
    // `cwd` only informs the macOS OS-sandbox `getcwd` grant below (Landlock and
    // AppContainer do not gate `getcwd`, and the grant is skipped when no OS
    // sandbox spec is present). Bind it so it is never flagged unused on the
    // targets/paths that do not consult it; `Option<&Path>` is `Copy`, so the
    // macOS grant can still read it afterward.
    let _ = cwd;
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

    // The base program for each runtime is normally the bare name on PATH, but a
    // confined Windows launch runs the *resolved* real binary directly. Any
    // runtime may be provided by a version-manager shim (nub, nvm, fnm, volta,
    // asdf, …), and such a shim resolves its version dynamically at startup.
    // Inside the AppContainer that resolution fails and the shim can fall back to
    // re-spawning the bare runtime name — which resolves back to the shim and
    // fork-storms until the launch times out. Launching the resolved binary (the
    // same path `add_runtime_essentials` grants) is deterministic and shim-free.
    // Bun keeps the bare name: it runs unconfined on Windows (it cannot boot
    // inside the container, see below), so the shim risk does not apply to it.
    let program: std::ffi::OsString = match runtime {
        DelegatingJsRuntimeOption::Node => "node".into(),
        DelegatingJsRuntimeOption::Bun => "bun".into(),
        DelegatingJsRuntimeOption::Deno => "deno".into(),
        DelegatingJsRuntimeOption::Auto => {
            unreachable!("Auto runtime resolved above")
        }
    };
    #[cfg(target_os = "windows")]
    let program = {
        fn canonicalized(p: PathBuf) -> PathBuf {
            std::fs::canonicalize(&p).unwrap_or(p)
        }
        let confined = spawn_policy.os_sandbox.is_some()
            && std::env::var_os("OMNI_DISABLE_OS_SANDBOX").is_none()
            && runtime != DelegatingJsRuntimeOption::Bun;
        let path_entry = runtime_bin(runtime)
            .and_then(|bin| which::which(bin).ok())
            .map(canonicalized);
        let resolved =
            crate::runtime::resolved_exec_path(runtime).map(canonicalized);
        match confined_launch_program(
            confined,
            path_entry.as_deref(),
            resolved.as_deref(),
        ) {
            ConfinedLaunchProgram::BareName => program,
            ConfinedLaunchProgram::RealBinary(real) => real.into_os_string(),
            ConfinedLaunchProgram::Degraded => {
                trace::warn!(
                    runtime = ?runtime,
                    "confined launch could not resolve the real runtime \
                     binary; falling back to the bare name, which may fail or \
                     fork-storm inside the sandbox — confinement is degraded \
                     on this host"
                );
                program
            }
        }
    };

    let mut command = match runtime {
        DelegatingJsRuntimeOption::Node => std::process::Command::new(&program),
        DelegatingJsRuntimeOption::Bun => {
            let mut c = std::process::Command::new(&program);
            c.arg("run");
            c
        }
        DelegatingJsRuntimeOption::Deno => {
            let mut c = std::process::Command::new(&program);
            c.arg("run");
            c
        }
        DelegatingJsRuntimeOption::Auto => {
            unreachable!("Auto runtime resolved above")
        }
    };

    // Capability-derived launch restrictions (replaces the old `--allow-all`).
    command.args(&spawn_policy.args);

    // When the policy permits spawning a child process, the shim resolves the
    // runtime via `Deno.execPath()` to launch the confined child. On Deno that
    // call requires `--allow-read` for the runtime binary, but the `process`
    // domain only lowers to `--allow-run`, so the spawn otherwise dies with
    // `Requires read access to <exec_path>`. Grant read of the resolved
    // executable path (safe: it is the runtime reading its own binary, the same
    // path `add_runtime_essentials` grants the OS sandbox).
    if runtime == DelegatingJsRuntimeOption::Deno
        && spawn_policy
            .args
            .iter()
            .any(|a| a == "--allow-run" || a.starts_with("--allow-run="))
        && let Some(exec) = crate::runtime::resolved_exec_path(runtime)
    {
        command.arg(format!(
            "--allow-read={}",
            exec.to_string_lossy().replace('\\', "/")
        ));
    }
    // On Windows the OS sandbox confines the runtime inside an AppContainer,
    // which runs at *Low* integrity and therefore cannot write the Medium
    // integrity workspace. Deno would otherwise try to materialise a `deno.lock`
    // / a local `node_modules` in the (workspace) context dir at startup and die
    // with "Access is denied (os error 5)" before running anything. The bridge is
    // a self-contained bundle and every script filesystem write is brokered over
    // RPC (never issued directly by the runtime), so it needs neither: suppress
    // both so the confined runtime never writes the workspace to boot.
    #[cfg(target_os = "windows")]
    if spawn_policy.os_sandbox.is_some() {
        match runtime {
            DelegatingJsRuntimeOption::Deno => {
                command.arg("--no-lock");
                command.arg("--node-modules-dir=none");
            }
            DelegatingJsRuntimeOption::Node => {
                // Node's module resolver `realpath`s every module it loads
                // (`resolveMainPath` -> `toRealPath` for the entry, and the
                // loader for each dependency), which `lstat`s every ancestor up
                // to the drive root `C:\`. A Low-integrity AppContainer cannot
                // `lstat` `C:\` and the launch dies with `EPERM ... lstat 'C:\'`
                // before any script runs. `--preserve-symlinks(-main)` skips the
                // realpath walk entirely; the bundle is self-contained so its
                // module identity does not depend on symlink resolution.
                command.arg("--preserve-symlinks");
                command.arg("--preserve-symlinks-main");
            }
            _ => {}
        }
    }
    command.arg(entrypoint);
    command.args(script_args);

    // Scrub the child's ambient environment when the policy layer governs `env`.
    // Without this the runtime would inherit omni's *entire* environment, so a
    // generator script could read any variable (e.g. an ambient cloud token)
    // through the un-mediated `process.env` / `Deno.env`, bypassing the `env`
    // capability. Policy-allowed variables are supplied explicitly; everything
    // else is dropped except the fixed runtime bootstrap set.
    if let Some(policy_env) = &spawn_policy.env {
        apply_scrubbed_env(&mut command, policy_env);
    }

    // Tier-3 OS sandbox. On Linux this installs a `pre_exec` Landlock hook onto
    // the command (carried over into the async spawn). On Windows the sandbox
    // cannot be attached to a `Command` for a later spawn, so the call only
    // validates that confinement is establishable (failing closed otherwise) and
    // the augmented spec is returned for the confined spawn to apply.
    //
    // The policy's spec confines the *script's* filesystem authority, but the
    // sandbox binds the whole child — including the runtime itself — so it must
    // also be granted the paths the runtime needs merely to start and run:
    // its own executable directory and its module/compile cache. Without these
    // the sandbox would deny the runtime reading its own binary or writing its
    // cache, and the spawn would fail before any script executed.
    #[allow(unused_mut)]
    let mut confinement: Option<OsSandboxSpec> = None;
    let mut sandbox_temp_dir: Option<SandboxTempDir> = None;
    if let Some(spec) = &spawn_policy.os_sandbox {
        let mut spec = spec.clone();
        let sandbox_temp = add_runtime_essentials(runtime, &mut spec);
        // macOS Seatbelt gates `getcwd(2)`: the confined runtime reads its own
        // working directory during startup (Deno/Node both call `getcwd`), so
        // the cwd must be a readable root under the profile or the launch dies
        // with "could not read current working directory (os error 1)" before
        // any script runs. Landlock (Linux) and AppContainer (Windows) do not
        // gate `getcwd`, so this grant is macOS-only — it does not widen the
        // floor on the other platforms. The brokered `sys` reads stay enforced
        // against the policy regardless; this only lets the runtime resolve the
        // directory it is launched in. Falls back to the inherited cwd when the
        // caller sets none.
        //
        // The path is **canonicalized** before granting: Seatbelt matches
        // `(subpath …)` against the *resolved* path, and macOS temp dirs live
        // under `/var/folders/…` where `/var` is a symlink to `/private/var`, so
        // granting the raw path would never match the child's resolved cwd and
        // `getcwd` would stay denied. Falls back to the raw path if the resolve
        // fails.
        #[cfg(target_os = "macos")]
        if let Some(dir) = cwd
            .map(std::path::Path::to_path_buf)
            .or_else(|| std::env::current_dir().ok())
        {
            let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
            if !spec.read_paths.contains(&dir) {
                spec.read_paths.push(dir);
            }
        }
        // Redirect the confined child's temp dir to the granted per-run
        // directory (see `add_runtime_essentials`). Set after `apply_scrubbed_env`
        // so it overrides any `TEMP`/`TMP` forwarded from the parent, keeping the
        // runtime off the shared system temp the sandbox does not grant.
        if let Some(dir) = &sandbox_temp {
            command.env("TEMP", dir);
            command.env("TMP", dir);
            command.env("TMPDIR", dir);
        }
        // Own the per-run temp dir so it is removed when the runner is dropped
        // rather than leaking one directory into `%TEMP%` per confined spawn.
        sandbox_temp_dir = sandbox_temp.map(SandboxTempDir);
        omni_capability_enforcement::install_os_sandbox(&mut command, &spec)
            .map_err(|e| {
                error::error!("failed to install the OS sandbox: {e}")
            })?;
        // On Windows the sandbox is applied at spawn time via AppContainer, so
        // carry the spec forward for the confined spawn — but honour the same
        // `OMNI_DISABLE_OS_SANDBOX` escape hatch `install_os_sandbox` respects on
        // the other platforms, launching unconfined when it is set (the broker
        // still mediates every operation).
        //
        // Bun is excluded: it reads and `realpath`s its current working
        // directory during startup, which `stat`s every ancestor up to the drive
        // root `C:\`. A Low-integrity AppContainer cannot stat `C:\` (nor the
        // un-granted ancestors between it and the workspace) and bun aborts with
        // `CouldntReadCurrentDirectory` before any script runs. Unlike Node
        // (`--preserve-symlinks`) and Deno, bun exposes no flag to skip that
        // walk, so it cannot boot inside the container. It therefore runs
        // unconfined on Windows with the broker/shim still mediating every
        // operation — the same enforcement bun receives on every platform, since
        // it has no native permission flags anywhere (documented weaker
        // AppContainer guarantee).
        #[cfg(target_os = "windows")]
        if std::env::var_os("OMNI_DISABLE_OS_SANDBOX").is_none()
            && runtime != DelegatingJsRuntimeOption::Bun
        {
            confinement = Some(spec);
        }
    }

    Ok((command, confinement, sandbox_temp_dir))
}

/// Environment variables the JS runtime (and the common version-manager shims
/// that re-`execve` it) need merely to *start*, independent of the capability
/// policy. When a [`SpawnPolicy`] supplies an explicit (scrubbed) child
/// environment, the ambient environment is otherwise cleared, so these must be
/// forwarded from the parent or the runtime may fail to locate its binary or
/// caches. Several of them (`TMPDIR`, `DENO_DIR`, `BUN_INSTALL`, …) must match
/// what the child actually uses, because [`add_runtime_essentials`] grants the
/// Landlock fs paths derived from them.
///
/// This is deliberately an **allow-list**: a deny-list would leak every new
/// ambient variable. It also deliberately excludes code-injection vectors
/// (`NODE_OPTIONS`, `LD_PRELOAD`, `DYLD_*`, …) so a scrubbed launch cannot be
/// turned into arbitrary code execution via the environment.
const RUNTIME_BOOTSTRAP_ENV: &[&str] = &[
    // Locating binaries, home, temp dir, locale, shell.
    "PATH",
    "HOME",
    "TMPDIR",
    "TMP",
    "TEMP",
    "TERM",
    "TZ",
    "LANG",
    "LANGUAGE",
    "LC_ALL",
    "LC_CTYPE",
    "USER",
    "LOGNAME",
    "SHELL",
    "XDG_CACHE_HOME",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    // Runtime module/compile caches and install roots. Must agree with the
    // paths `add_runtime_essentials` grants under Landlock.
    "DENO_DIR",
    "DENO_INSTALL",
    "DENO_INSTALL_ROOT",
    "BUN_INSTALL",
    "NODE_V8_COVERAGE",
    // Version managers that re-exec the real runtime binary from elsewhere.
    "NVM_DIR",
    "NVM_BIN",
    "NUB_HOME",
    "FNM_DIR",
    "FNM_MULTISHELL_PATH",
    "VOLTA_HOME",
    "N_PREFIX",
    "ASDF_DIR",
    "ASDF_DATA_DIR",
    "PNPM_HOME",
    "COREPACK_HOME",
    // Windows essentials (loader, user dirs, executable resolution).
    "SYSTEMROOT",
    "WINDIR",
    "APPDATA",
    "LOCALAPPDATA",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "PATHEXT",
    "COMSPEC",
    "PROCESSOR_ARCHITECTURE",
    "NUMBER_OF_PROCESSORS",
];

/// Linker/loader and code-injection environment variables that must **never**
/// reach a confined child, even when the `env` policy allows them (e.g.
/// `env: ["*"]`). Forwarding one would let the child — and every process it
/// spawns — be hijacked into loading attacker-controlled code (a writable
/// directory on `LD_LIBRARY_PATH`, a preloaded `.so`/`.dylib`, injected
/// `NODE_OPTIONS`), defeating the sandbox entirely. The [`RUNTIME_BOOTSTRAP_ENV`]
/// allow-list already omits these; this denylist extends the same rule over
/// *policy-supplied* variables, which would otherwise pass through verbatim.
///
/// It is also exactly the set Deno refuses to spawn with under a *scoped*
/// `--allow-run`, so scrubbing them here lets a confined generator run a shell
/// under a precise `process` grant instead of needing a blanket one.
const ENV_INJECTION_DENYLIST: &[&str] = &[
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "LD_AUDIT",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "DYLD_FRAMEWORK_PATH",
    "DYLD_FALLBACK_LIBRARY_PATH",
    "DYLD_FALLBACK_FRAMEWORK_PATH",
    "NODE_OPTIONS",
];

/// Whether `key` names a code-injection vector that must be dropped from a
/// confined child's environment regardless of policy (see
/// [`ENV_INJECTION_DENYLIST`]). Compared case-insensitively so a
/// case-insensitive OS cannot be used to smuggle one past the check.
fn is_env_injection_vector(key: &str) -> bool {
    ENV_INJECTION_DENYLIST
        .iter()
        .any(|denied| denied.eq_ignore_ascii_case(key))
}

/// The scrubbed environment a confined child launches with: the runtime
/// bootstrap variables that are actually set in the parent, followed by the
/// caller's `policy_env` (which wins on a name clash). This is a pure function of
/// the parent environment and the policy so it can be asserted in tests; any
/// ambient variable not on the bootstrap allow-list and not policy-allowed is
/// absent from the result and therefore never reaches the child.
///
/// Code-injection vectors ([`ENV_INJECTION_DENYLIST`]) are dropped **even when
/// `policy_env` supplies them** (e.g. an `env: ["*"]` snapshot that swept up the
/// ambient `LD_LIBRARY_PATH`): a confined child must never be launchable with a
/// linker-hijack variable in its environment.
fn scrubbed_child_env(
    policy_env: &BTreeMap<String, String>,
) -> Vec<(String, std::ffi::OsString)> {
    let mut out: Vec<(String, std::ffi::OsString)> = Vec::new();
    for &key in RUNTIME_BOOTSTRAP_ENV {
        if let Some(value) = std::env::var_os(key) {
            out.push((key.to_string(), value));
        }
    }
    for (key, value) in policy_env {
        // A policy allowance can never re-introduce a linker/code-injection
        // vector the sandbox is built to exclude.
        if is_env_injection_vector(key) {
            continue;
        }
        out.push((key.clone(), std::ffi::OsString::from(value)));
    }
    out
}

/// Replace the child's inherited environment with the fixed runtime bootstrap
/// set (forwarded from the parent where present) plus the `policy_env` the
/// caller explicitly allowed. Any other ambient variable is dropped, so a
/// policy-denied variable never reaches the confined runtime's `process.env`.
fn apply_scrubbed_env(
    command: &mut std::process::Command,
    policy_env: &BTreeMap<String, String>,
) {
    command.env_clear();
    for (key, value) in scrubbed_child_env(policy_env) {
        command.env(key, value);
    }
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
) -> Option<PathBuf> {
    let bin = match runtime {
        DelegatingJsRuntimeOption::Node => "node",
        DelegatingJsRuntimeOption::Bun => "bun",
        DelegatingJsRuntimeOption::Deno => "deno",
        // `Auto` is resolved before this point; nothing to add otherwise.
        DelegatingJsRuntimeOption::Auto => return None,
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
    // The canonical path the runtime actually runs from, resolved below and
    // reused by the per-runtime arm (e.g. to detect a nub-managed Node).
    let mut real_exec_path: Option<PathBuf> = None;
    if let Some(real) = crate::runtime::resolved_exec_path(runtime) {
        // `process.execPath`/`Deno.execPath()` can report a version-manager
        // *junction* (e.g. fnm's per-shell `fnm_multishells\<id>\node.exe`)
        // whose parent is a shared root holding every shell's dir. Granting that
        // parent as the install root is both over-broad and — because the
        // subtree-inheritable ACE re-propagates to all its children — as slow as
        // granting the shared temp dir was (thousands of entries, seconds per
        // spawn). Resolve the junction to the real binary first so the install
        // root is the runtime's own (small) version tree.
        let real = std::fs::canonicalize(&real).unwrap_or(real);
        push_parent(&mut spec.read_paths, &real);
        if let Some(bin_dir) = real.parent()
            && let Some(install_root) = bin_dir.parent()
        {
            spec.read_paths.push(install_root.to_path_buf());
        }
        real_exec_path = Some(real);
    }

    // Runtimes stage temporary files; grant a writable temp directory. On
    // Windows, granting the *shared* system temp dir is catastrophically slow:
    // applying an inheritable ACE propagates it across every existing entry in
    // `%TEMP%` (tens of thousands on a dev box), adding ~30s to every confined
    // spawn. Use a fresh, empty per-run subdirectory instead (O(1) to grant) and
    // point the child's temp env at it (see `build_command`). Other platforms
    // add a single Landlock path rule with no tree walk, so the system temp dir
    // is granted directly there.
    let sandbox_temp = provision_sandbox_temp();
    match &sandbox_temp {
        Some(dir) => ensure_writable(&mut spec.write_paths, dir.clone()),
        None => ensure_writable(&mut spec.write_paths, std::env::temp_dir()),
    }

    // Grant read/execute for the directory of every program the policy allows
    // the script to spawn, so the confined child can `execve` it. Names in a
    // directory already covered by the sandbox baseline (e.g. `/usr/bin`) are
    // harmlessly re-added. Version-manager shims are common, so a resolved
    // symlink's real target directory is granted too.
    //
    // Windows is the exception: its sandbox is applied at *spawn* time
    // (AppContainer), and program resolution there is materially harder — a bare
    // name may resolve to a different `PATH` entry than `which` picks, package
    // managers add non-symlink shim launchers (scoop) and junctioned install
    // trees, so a single `which`+`canonicalize` under-grants and the confined
    // child hits `os error 5` launching the real binary. So the exec programs
    // are left on the spec for the AppContainer backend to resolve generously at
    // spawn time (see `appcontainer_sandbox::program_dirs`); the pre-spawn
    // backends (Landlock/Seatbelt) still need them lowered into `read_paths`
    // here.
    #[cfg(not(target_os = "windows"))]
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
            // `DENO_DIR` (module/compile cache) is written at runtime. Use the
            // same per-platform default Deno itself picks when it is unset.
            if let Some(dir) = deno_cache_dir() {
                ensure_writable(&mut spec.write_paths, dir);
            }
            // Global Deno config / install root (`DENO_INSTALL_ROOT`, else
            // `<home>/.deno` on every platform Deno's installer targets).
            let install_root = std::env::var_os("DENO_INSTALL_ROOT")
                .map(PathBuf::from)
                .or_else(|| home_dir().map(|h| h.join(".deno")));
            if let Some(root) = install_root {
                spec.read_paths.push(root);
            }
        }
        DelegatingJsRuntimeOption::Bun => {
            // Bun reads its runtime files and writes its module cache under its
            // install root (`%USERPROFILE%\.bun` on Windows, `~/.bun`
            // elsewhere, or an explicit `BUN_INSTALL`).
            let install = std::env::var_os("BUN_INSTALL")
                .map(PathBuf::from)
                .or_else(|| home_dir().map(|h| h.join(".bun")));
            if let Some(dir) = install {
                ensure_writable(&mut spec.write_paths, dir);
            }
        }
        DelegatingJsRuntimeOption::Node => {
            // Node executing a prebuilt bundle needs no writable cache, and its
            // libraries live under system prefixes already in the baseline. But
            // when Node is provisioned by `nub`, the `node` on PATH is a shim
            // that re-resolves the version on *every* launch: it enumerates the
            // installed versions under its cache and loads its own preload +
            // native-addon files from `<cache>/runtime-*/` before re-`execve`ing
            // the real binary. `process.execPath` only reveals that one version
            // dir (granted above), so under the sandbox the shim cannot read the
            // rest of its cache, concludes there is "no Node", and tries to
            // provision the latest into the (unwritable) cache — failing the
            // launch. Grant the nub cache root read/execute so an already
            // installed runtime resolves. Read-only on purpose: a resolved
            // launch performs no writes, and provisioning from inside the
            // sandbox stays disabled.
            if let Some(cache) =
                real_exec_path.as_deref().and_then(nub_cache_root)
            {
                spec.read_paths.push(cache);
            }
        }
        DelegatingJsRuntimeOption::Auto => {}
    }

    sandbox_temp
}

/// A fresh, empty per-run temp directory to grant a confined child instead of
/// the shared system temp dir. Returns `None` where the OS sandbox grants a path
/// in O(1) (Landlock), so the system temp dir is used directly; `Some(dir)` on
/// Windows, where granting the shared `%TEMP%` would force an inheritable-ACE
/// walk across all of its existing entries on every spawn.
///
/// The directory name uses an unpredictable suffix and is created **exclusively**
/// (`create_dir`, which fails if the path already exists) rather than a
/// predictable `omni-sandbox-<pid>-<seq>` via `create_dir_all`. A predictable
/// name under the world-writable system temp is a local pre-creation vector: an
/// attacker could pre-create it as a junction/symlink so the child's temp writes
/// land somewhere they control. Exclusive create rejects a squatted name (we
/// retry with a fresh suffix), and the entropy makes guessing the next name
/// impractical.
#[cfg(windows)]
fn provision_sandbox_temp() -> Option<PathBuf> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);

    // Best-effort, once-per-process sweep of temp dirs leaked by earlier runs
    // that crashed before their `SandboxTempDir` guard could remove them.
    sweep_stale_sandbox_temps();

    let base = std::env::temp_dir();
    for _ in 0..16 {
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = base.join(format!(
            "omni-sandbox-{}-{}",
            std::process::id(),
            random_temp_suffix(seq)
        ));
        match std::fs::create_dir(&dir) {
            Ok(()) => return Some(dir),
            // A collision (attacker squat or an astronomically-unlikely suffix
            // clash) — try a fresh suffix rather than adopting the existing dir.
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return None,
        }
    }
    None
}

/// An unpredictable, filesystem-safe suffix for a per-run temp dir name. Derived
/// by hashing the pid, a per-process sequence, and a high-resolution timestamp;
/// the exclusive `create_dir` is the actual squat guard, so this only needs to
/// be hard to *predict*, not cryptographically random.
#[cfg(windows)]
fn random_temp_suffix(seq: u64) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut hasher = blake3::Hasher::new();
    hasher.update(&std::process::id().to_le_bytes());
    hasher.update(&seq.to_le_bytes());
    hasher.update(&nanos.to_le_bytes());
    hasher.finalize().to_hex()[..16].to_string()
}

/// Remove `omni-sandbox-*` temp dirs left behind by earlier runs. Runs once per
/// process and is strictly best-effort: it only removes entries older than a
/// conservative threshold so a directory in active use by a concurrently-running
/// omni (or a legitimately long-running confined spawn) is never touched.
#[cfg(windows)]
fn sweep_stale_sandbox_temps() {
    use std::sync::Once;
    static SWEPT: Once = Once::new();
    SWEPT.call_once(|| {
        // Old enough that an in-flight run cannot plausibly own it, but short
        // enough that leaked dirs do not accumulate across a work session.
        const STALE_AGE: Duration = Duration::from_secs(6 * 60 * 60);
        let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if !name.starts_with("omni-sandbox-") {
                continue;
            }
            let stale = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .map(|age| age >= STALE_AGE)
                .unwrap_or(false);
            if stale {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    });
}

#[cfg(not(windows))]
fn provision_sandbox_temp() -> Option<PathBuf> {
    None
}

/// Owns a per-run sandbox temp directory, removing it (recursively) on drop.
///
/// A confined child stages its temp files in a fresh per-run directory rather
/// than the shared system temp (see [`provision_sandbox_temp`]). Tying that
/// directory's lifetime to the runner keeps `%TEMP%` from accumulating one
/// abandoned directory per confined spawn: when the runner is dropped, the
/// directory (and anything the child left in it) is removed. Removal is
/// best-effort — a lingering handle from a not-yet-reaped child only defers the
/// cleanup to the OS, it never fails the run.
struct SandboxTempDir(PathBuf);

impl Drop for SandboxTempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
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

/// The current user's home directory. On Windows the runtime install roots and
/// caches hang off `%USERPROFILE%` (Git-Bash-style `HOME` may be absent or hold
/// a non-native `/c/...` path), so prefer it there and fall back to
/// `HOMEDRIVE`+`HOMEPATH`; on other platforms use `HOME`.
fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        if let Some(profile) = std::env::var_os("USERPROFILE")
            && !profile.is_empty()
        {
            return Some(PathBuf::from(profile));
        }
        let drive = std::env::var_os("HOMEDRIVE")?;
        let path = std::env::var_os("HOMEPATH")?;
        let mut home = PathBuf::from(drive);
        home.push(path);
        Some(home)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

/// The Deno module/compile cache directory (`DENO_DIR`): an explicit `DENO_DIR`
/// when set, otherwise the per-platform default Deno itself chooses
/// (`%LOCALAPPDATA%\deno` on Windows, `~/Library/Caches/deno` on macOS,
/// `$XDG_CACHE_HOME/deno` or `~/.cache/deno` elsewhere). The confined runtime
/// must be able to write here or it fails before running any script.
fn deno_cache_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("DENO_DIR")
        && !dir.is_empty()
    {
        return Some(PathBuf::from(dir));
    }
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA")
            .filter(|p| !p.is_empty())
            .map(|p| PathBuf::from(p).join("deno"))
            .or_else(|| {
                home_dir().map(|h| h.join("AppData").join("Local").join("deno"))
            })
    }
    #[cfg(target_os = "macos")]
    {
        home_dir().map(|h| h.join("Library/Caches/deno"))
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        std::env::var_os("XDG_CACHE_HOME")
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .or_else(|| home_dir().map(|h| h.join(".cache")))
            .map(|c| c.join("deno"))
    }
}

/// nub's cache root derived from the *resolved* runtime binary, or `None` when
/// the binary is not nub-managed. nub keeps every installed Node version under
/// `<cache>/nub/node/<ver>/` and its launch-time preload + native-addon shim
/// under `<cache>/nub/runtime-*/`; a confined child spawned through the nub shim
/// must read both to resolve and re-exec an already-installed runtime (see
/// `add_runtime_essentials` for why the single `process.execPath` version dir is
/// not enough).
///
/// The version binary sits at `<cache>/nub/node/<ver>/node.exe` on Windows but
/// `<cache>/nub/node/<ver>/bin/node` on Unix, so the distance from the executable
/// to the `node` version store differs by platform. Rather than assume a fixed
/// depth, walk the ancestry for a `node` directory whose parent is the `nub`
/// cache root and return that root. A system or other-version-manager Node
/// (e.g. `/usr/bin/node`, `~/.nvm/versions/node/<ver>/bin/node`) has no such
/// `nub/node` pair and is correctly left ungranted.
fn nub_cache_root(real_exec: &Path) -> Option<PathBuf> {
    let mut dir = real_exec.parent();
    while let Some(candidate) = dir {
        if candidate.file_name().is_some_and(|n| n == "node")
            && let Some(cache) = candidate.parent()
            && cache.file_name().is_some_and(|n| n == "nub")
        {
            return Some(cache.to_path_buf());
        }
        dir = candidate.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{CallRace, race_call};
    use super::{ConfinedLaunchProgram, Path, confined_launch_program};
    use super::{PathBuf, SandboxTempDir};
    use super::{RUNTIME_BOOTSTRAP_ENV, nub_cache_root, scrubbed_child_env};

    #[test]
    fn an_unconfined_launch_uses_the_bare_name() {
        // With no confinement the resolved/shim distinction is irrelevant: the
        // bare `PATH` name is launched exactly as before.
        assert_eq!(
            confined_launch_program(
                false,
                Some(Path::new("/usr/bin/deno")),
                Some(Path::new("/opt/deno/bin/deno")),
            ),
            ConfinedLaunchProgram::BareName,
        );
    }

    #[test]
    fn a_direct_binary_uses_the_bare_name() {
        // The runtime on `PATH` *is* the real binary (execPath == PATH entry),
        // so there is no shim to bypass and no override is applied.
        let p = Path::new("/usr/bin/deno");
        assert_eq!(
            confined_launch_program(true, Some(p), Some(p)),
            ConfinedLaunchProgram::BareName,
        );
    }

    #[test]
    fn a_shim_launches_the_resolved_real_binary() {
        // execPath differs from the `PATH` entry: the entry is a shim, so the
        // resolved real binary is launched directly under confinement.
        let entry = Path::new("/home/u/.nvm/shims/node");
        let real = Path::new("/home/u/.nvm/versions/v20/bin/node");
        assert_eq!(
            confined_launch_program(true, Some(entry), Some(real)),
            ConfinedLaunchProgram::RealBinary(real.to_path_buf()),
        );
    }

    #[test]
    fn a_resolved_binary_with_no_path_entry_is_launched_directly() {
        // `which` found nothing but the runtime reported its own path: launch
        // that absolute path rather than a bare name that cannot be resolved.
        let real = Path::new("/opt/deno/bin/deno");
        assert_eq!(
            confined_launch_program(true, None, Some(real)),
            ConfinedLaunchProgram::RealBinary(real.to_path_buf()),
        );
    }

    #[test]
    fn an_unresolvable_real_binary_is_degraded() {
        // No self-reported path: confinement is degraded and the caller warns,
        // whether or not a `PATH` entry exists.
        assert_eq!(
            confined_launch_program(
                true,
                Some(Path::new("/usr/bin/deno")),
                None,
            ),
            ConfinedLaunchProgram::Degraded,
        );
        assert_eq!(
            confined_launch_program(true, None, None),
            ConfinedLaunchProgram::Degraded,
        );
    }

    #[test]
    fn nub_cache_root_is_derived_from_the_binary_layout_cross_platform() {
        // The nub cache root is the `nub` dir whose child is the `node` version
        // store, found regardless of where the OS puts the cache, what the exe
        // is named, or whether the version tree has a `bin/` level. Unix keeps
        // the binary under `<ver>/bin/`; Windows keeps it directly at `<ver>/`.
        for real in [
            "/home/u/.cache/nub/node/26.5.0/bin/node",
            "/Users/u/Library/Caches/nub/node/26.5.0/bin/node",
            // Forward slashes are valid separators on Windows too, so this
            // stays a single cross-platform assertion (a backslash literal
            // would not decompose on a Unix `Path`).
            "C:/Users/u/AppData/Local/nub/node/26.5.0/bin/node.exe",
            // The real Windows layout: no `bin/` level — `node.exe` sits at the
            // version root.
            "C:/Users/u/.cache/nub/node/26.5.0/node.exe",
        ] {
            let root = nub_cache_root(&PathBuf::from(real));
            assert_eq!(
                root.as_ref().and_then(|p| p.file_name()),
                Some(std::ffi::OsStr::new("nub")),
                "nub layout must resolve to its `nub` cache root: {real}"
            );
        }

        // A system or other-VM Node must never match — granting its derived
        // ancestor would over-expose (e.g. `/` for `/usr/bin/node`).
        for non_nub in [
            "/usr/bin/node",
            "/home/u/.nvm/versions/node/v22.0.0/bin/node",
            "/home/u/.cache/other/node/26.5.0/bin/node",
        ] {
            assert_eq!(
                nub_cache_root(&PathBuf::from(non_nub)),
                None,
                "non-nub runtime must not be treated as nub-managed: {non_nub}"
            );
        }
    }

    #[test]
    fn scrubbed_env_contains_only_bootstrap_and_policy_vars() {
        let mut policy = BTreeMap::new();
        policy.insert("OMNI_TEST_ALLOWED".to_string(), "yes".to_string());
        let env = scrubbed_child_env(&policy);

        // The policy-allowed variable passes through with its value.
        assert!(
            env.iter().any(|(k, v)| k == "OMNI_TEST_ALLOWED"
                && v == std::ffi::OsStr::new("yes")),
            "policy-allowed variable must reach the child"
        );

        // Crucially, every entry is either a fixed bootstrap key or the
        // policy-allowed variable — no arbitrary inherited ambient variable can
        // leak into the child (this is the property that closes the
        // `process.env` env-capability bypass).
        for (key, _) in &env {
            let allowed = RUNTIME_BOOTSTRAP_ENV.contains(&key.as_str())
                || policy.contains_key(key);
            assert!(
                allowed,
                "unexpected ambient variable leaked into the child env: {key}"
            );
        }
    }

    #[test]
    fn scrubbed_env_forwards_present_bootstrap_vars() {
        // `PATH` is a bootstrap key and is effectively always set in a test
        // environment, so a scrubbed launch must still forward it or the
        // runtime could not locate its own binary.
        if std::env::var_os("PATH").is_some() {
            let env = scrubbed_child_env(&BTreeMap::new());
            assert!(
                env.iter().any(|(k, _)| k == "PATH"),
                "PATH must be forwarded to the confined child"
            );
        }
    }

    #[test]
    fn scrubbed_env_excludes_code_injection_vectors() {
        // A defense-in-depth guard: the bootstrap allow-list must never carry
        // an env var that can inject code into the runtime, or a scrubbed
        // launch could be turned into arbitrary execution via the environment.
        for vector in [
            "NODE_OPTIONS",
            "LD_PRELOAD",
            "LD_LIBRARY_PATH",
            "DYLD_INSERT_LIBRARIES",
            "DYLD_LIBRARY_PATH",
        ] {
            assert!(
                !RUNTIME_BOOTSTRAP_ENV.contains(&vector),
                "{vector} must not be in the runtime bootstrap allow-list"
            );
        }
    }

    #[test]
    fn scrubbed_env_drops_injection_vectors_even_when_policy_allows_them() {
        // The critical property: a permissive `env` policy (e.g. `env: ["*"]`)
        // whose snapshot swept up the ambient `LD_LIBRARY_PATH` must NOT be able
        // to re-introduce a linker/code-injection vector into the confined
        // child. Ordinary policy vars still pass through.
        let mut policy = BTreeMap::new();
        policy
            .insert("LD_LIBRARY_PATH".to_string(), "/tmp/evil/lib".to_string());
        policy.insert(
            "DYLD_INSERT_LIBRARIES".to_string(),
            "/tmp/evil.dylib".to_string(),
        );
        policy.insert("NODE_OPTIONS".to_string(), "--require=/x".to_string());
        // Case-insensitive smuggling attempt must also be dropped.
        policy.insert("ld_preload".to_string(), "/tmp/x.so".to_string());
        policy.insert("OMNI_TEST_OK".to_string(), "1".to_string());

        let env = scrubbed_child_env(&policy);

        for vector in [
            "LD_LIBRARY_PATH",
            "DYLD_INSERT_LIBRARIES",
            "NODE_OPTIONS",
            "ld_preload",
        ] {
            assert!(
                !env.iter().any(|(k, _)| k == vector),
                "policy-allowed injection vector {vector} must be dropped from \
                 the confined child env"
            );
        }
        assert!(
            env.iter()
                .any(|(k, v)| k == "OMNI_TEST_OK"
                    && v == std::ffi::OsStr::new("1")),
            "an ordinary policy-allowed variable must still reach the child"
        );
    }

    /// A unique scratch directory under the system temp dir for a cleanup test.
    fn scratch(tag: &str) -> PathBuf {
        let unique = format!(
            "omni-sandbox-test-{}-{}-{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        std::env::temp_dir().join(unique)
    }

    #[test]
    fn sandbox_temp_dir_is_removed_with_its_contents_on_drop() {
        // The per-run sandbox temp dir must not linger in `%TEMP%` after the
        // runner that owns it goes away, even when the (confined) child left
        // files behind in it.
        let dir = scratch("drop");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("staged.tmp"), b"child temp data").unwrap();
        std::fs::create_dir_all(dir.join("nested")).unwrap();
        std::fs::write(dir.join("nested/inner.tmp"), b"more").unwrap();
        assert!(dir.exists(), "precondition: the scratch dir exists");

        let guard = SandboxTempDir(dir.clone());
        drop(guard);

        assert!(
            !dir.exists(),
            "dropping the guard must remove the temp dir and its contents"
        );
    }

    #[test]
    fn sandbox_temp_dir_drop_tolerates_a_missing_directory() {
        // Cleanup is best-effort: if the directory is already gone (e.g. the OS
        // reaped it, or it was never created), dropping the guard must not
        // panic.
        let dir = scratch("missing");
        assert!(!dir.exists(), "precondition: the dir does not exist");
        let guard = SandboxTempDir(dir.clone());
        drop(guard); // must not panic
        assert!(!dir.exists());
    }

    #[cfg(windows)]
    #[test]
    fn provision_sandbox_temp_yields_distinct_existing_directories() {
        use super::provision_sandbox_temp;
        // On Windows a confined spawn provisions a fresh, empty temp dir each
        // time; two provisions must not collide (so concurrent spawns cannot
        // clobber or leak into each other's temp).
        let a =
            provision_sandbox_temp().expect("windows provisions a temp dir");
        let b =
            provision_sandbox_temp().expect("windows provisions a temp dir");
        assert_ne!(a, b, "each provision must be a distinct directory");
        assert!(a.is_dir() && b.is_dir(), "provisioned dirs must exist");
        // Wrapping them in the guard both proves they clean up and avoids
        // leaking the test's own scratch dirs.
        drop(SandboxTempDir(a.clone()));
        drop(SandboxTempDir(b.clone()));
        assert!(!a.exists() && !b.exists(), "guards must remove both dirs");
    }

    #[cfg(windows)]
    #[test]
    fn provision_sandbox_temp_rejects_a_squatted_name() {
        // The exclusive `create_dir` is the guard against a pre-created
        // (potentially symlinked) name: provisioning must never adopt a
        // directory that already exists. We cannot force the exact next suffix,
        // so assert the invariant that every provisioned dir was freshly made by
        // us — i.e. it is empty at hand-off (a squatter's dir would carry
        // contents / not be ours).
        let dir =
            super::provision_sandbox_temp().expect("windows provisions a dir");
        let empty = std::fs::read_dir(&dir)
            .map(|mut e| e.next().is_none())
            .unwrap_or(false);
        assert!(
            empty,
            "a freshly provisioned sandbox temp dir must be empty"
        );
        drop(SandboxTempDir(dir));
    }

    #[tokio::test]
    async fn race_call_returns_completed_when_the_call_finishes_first() {
        // Keep `_tx` bound so the exit arm never fires; the ready call wins.
        let (_tx, mut rx) = tokio::sync::watch::channel(None);
        let out = race_call(
            async { 7 },
            &mut rx,
            Some(super::Duration::from_secs(10)),
        )
        .await;
        assert!(matches!(out, CallRace::Completed(7)));
    }

    #[tokio::test]
    async fn race_call_reports_child_exit_when_the_process_dies_first() {
        let (tx, mut rx) = tokio::sync::watch::channel(None);
        tx.send(Some(42)).unwrap();
        // The call never completes, so only the already-signalled exit can win.
        let out = race_call(std::future::pending::<i32>(), &mut rx, None).await;
        assert!(matches!(out, CallRace::ChildExited(42)));
    }

    #[tokio::test]
    async fn race_call_times_out_a_hung_call() {
        // The real "unusable in actual usage" case: neither the call nor the
        // child ever resolves, so without the timeout arm this would hang.
        let (_tx, mut rx) = tokio::sync::watch::channel(None);
        let out = race_call(
            std::future::pending::<i32>(),
            &mut rx,
            Some(super::Duration::from_millis(20)),
        )
        .await;
        assert!(matches!(out, CallRace::TimedOut));
    }
}
