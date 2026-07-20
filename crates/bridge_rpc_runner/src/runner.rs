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

    // Scrub the child's ambient environment when the policy layer governs `env`.
    // Without this the runtime would inherit omni's *entire* environment, so a
    // generator script could read any variable (e.g. an ambient cloud token)
    // through the un-mediated `process.env` / `Deno.env`, bypassing the `env`
    // capability. Policy-allowed variables are supplied explicitly; everything
    // else is dropped except the fixed runtime bootstrap set.
    if let Some(policy_env) = &spawn_policy.env {
        apply_scrubbed_env(&mut command, policy_env);
    }

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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{RUNTIME_BOOTSTRAP_ENV, scrubbed_child_env};

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
}
