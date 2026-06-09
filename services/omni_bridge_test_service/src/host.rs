//! Host mode – spawn a child process and drive bridge RPC against it.
//!
//! The [`Host`] type is the high-level handle that integration tests should
//! use to communicate with a child process running this binary in
//! `client` mode (or any binary that speaks the same bridge protocol over
//! stdin/stdout).
//!
//! ```no_run
//! use std::time::Duration;
//! use omni_bridge_test_service::host::{Host, HostSpawnOptions};
//!
//! # async fn demo() -> omni_bridge_test_service::Result<()> {
//! let host = Host::spawn(HostSpawnOptions::default()).await?;
//! let _ok = host.ping(Duration::from_secs(1)).await?;
//! host.shutdown().await?;
//! # Ok(())
//! # }
//! ```

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use bridge_rpc_core::{BridgeRpc, ClientHandle, StreamTransport};
use bridge_rpc_router::Router;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::task::JoinHandle;

use crate::cli::HostArgs;
use crate::error::{Error, ErrorInner, Result};

/// Convenience alias for the transport used by the host side – it reads
/// from the child's stdout and writes to the child's stdin.
pub type HostTransport = StreamTransport<ChildStdout, ChildStdin>;

/// Configuration for spawning a child process and wiring up a [`Host`].
#[derive(Debug, Clone, Default)]
pub struct HostSpawnOptions {
    /// Path to the child executable. When `None`, the currently-running
    /// executable is used (i.e. the binary spawns itself).
    pub child_binary: Option<PathBuf>,

    /// Extra arguments passed to the child before the `client` subcommand
    /// is appended (unless [`HostSpawnOptions::skip_client_subcommand`] is
    /// `true`).
    pub child_args: Vec<String>,

    /// Skip injecting the `client` subcommand into the child invocation.
    /// Use this when the child binary already enters a bridge-server mode
    /// without needing a subcommand.
    pub skip_client_subcommand: bool,

    /// Optional duration to wait after spawning the child before resolving
    /// [`Host::spawn`]. Useful when the child needs a moment to initialize.
    pub warmup: Option<Duration>,
}

impl HostSpawnOptions {
    /// Convenience constructor returning the defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Override the child binary path.
    pub fn with_child_binary(mut self, path: impl Into<PathBuf>) -> Self {
        self.child_binary = Some(path.into());
        self
    }

    /// Append an extra argument to the child invocation.
    pub fn with_child_arg(mut self, arg: impl Into<String>) -> Self {
        self.child_args.push(arg.into());
        self
    }

    /// Append several extra arguments to the child invocation.
    pub fn with_child_args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.child_args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Skip injecting the `client` subcommand.
    pub fn skip_client_subcommand(mut self, skip: bool) -> Self {
        self.skip_client_subcommand = skip;
        self
    }

    /// Set a post-spawn warmup duration.
    pub fn with_warmup(mut self, warmup: Duration) -> Self {
        self.warmup = Some(warmup);
        self
    }
}

impl From<&HostArgs> for HostSpawnOptions {
    fn from(args: &HostArgs) -> Self {
        Self {
            child_binary: args.child_binary.clone(),
            child_args: args.child_args.clone(),
            skip_client_subcommand: args.no_client_subcommand,
            warmup: if args.warmup_ms == 0 {
                None
            } else {
                Some(args.warmup())
            },
        }
    }
}

/// A spawned child process plus an associated [`BridgeRpc`] running over
/// its stdin/stdout.
///
/// The bridge's run loop is spawned onto the current Tokio runtime and the
/// resulting [`JoinHandle`] is held by this struct so that the loop is
/// driven for as long as the host lives.
///
/// Drop semantics: dropping a [`Host`] aborts the bridge's run loop and the
/// child process becomes orphaned. Prefer calling [`Host::shutdown`] for an
/// orderly teardown.
pub struct Host {
    bridge: BridgeRpc<HostTransport, Router>,
    child: Child,
    run_handle: JoinHandle<bridge_rpc_core::BridgeRpcResult<()>>,
}

impl Host {
    /// Spawn a child process according to `options`, build a bridge over
    /// its stdin/stdout, and start the run loop.
    ///
    /// The host bridge is configured with an *empty* [`Router`] – use
    /// [`Host::spawn_with_router`] when the host needs to expose its own
    /// services to the child (for instance, a `/log` handler that consumes
    /// log frames the child sends back).
    pub async fn spawn(options: HostSpawnOptions) -> Result<Self> {
        Self::spawn_with_router(options, Router::new()).await
    }

    /// Spawn a child process according to `options`, but use the supplied
    /// `router` for the host-side bridge.
    ///
    /// This is useful when the child sends requests *back* to the host
    /// (e.g. log forwarding) and the host needs to expose handlers for
    /// those paths.
    pub async fn spawn_with_router(
        options: HostSpawnOptions,
        router: Router,
    ) -> Result<Self> {
        let binary = match options.child_binary.clone() {
            Some(path) => path,
            None => std::env::current_exe()
                .map_err(ErrorInner::from)
                .map_err(Error::from)?,
        };

        let mut command = Command::new(binary);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());

        for arg in &options.child_args {
            command.arg(arg);
        }

        if !options.skip_client_subcommand {
            command.arg("client");
        }

        // Make sure the child is killed if we drop the [`Child`] without
        // calling [`Host::shutdown`].
        command.kill_on_drop(true);

        let mut child = command.spawn().map_err(ErrorInner::from)?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::missing_child_stream("stdin"))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::missing_child_stream("stdout"))?;

        // The host transport: read from the child's stdout, write to the
        // child's stdin.
        let transport = StreamTransport::new(stdout, stdin);

        let bridge = BridgeRpc::new(transport, router);

        let run_handle = {
            let bridge = bridge.clone();
            tokio::spawn(async move { bridge.run().await })
        };

        if let Some(warmup) = options.warmup {
            tokio::time::sleep(warmup).await;
        }

        Ok(Self {
            bridge,
            child,
            run_handle,
        })
    }

    /// Borrow the underlying [`BridgeRpc`].
    pub fn bridge(&self) -> &BridgeRpc<HostTransport, Router> {
        &self.bridge
    }

    /// Get a [`ClientHandle`] for issuing RPC requests against the child.
    pub fn client_handle(&self) -> Arc<ClientHandle> {
        self.bridge.get_client_handle()
    }

    /// Send a ping frame to the child and wait up to `timeout` for the
    /// pong response.
    pub async fn ping(&self, timeout: Duration) -> Result<bool> {
        let pong = self.bridge.ping(timeout).await?;
        Ok(pong)
    }

    /// Cleanly shut down the bridge and wait for the child process to
    /// exit.
    ///
    /// This sends a `Close` frame, gives the bridge run loop a brief
    /// window to drain naturally, and then aborts the run task so any
    /// stuck in-flight host-side service handlers (e.g. a `/log`
    /// handler waiting on body frames that the child stopped sending)
    /// don't hold shutdown up. Then it drops the transport (closing the
    /// child's stdin pipe so any in-flight read in the child wakes up
    /// with EOF) and waits for the child process exit.
    pub async fn shutdown(self) -> Result<std::process::ExitStatus> {
        let Self {
            bridge,
            mut child,
            run_handle,
        } = self;

        // Best-effort close: if the bridge has already stopped this will
        // fail, which is fine.
        let _ = bridge.close().await;

        // Try to drive the run loop to completion, but bound how long we
        // wait. If a host-side service task got stuck waiting on body
        // frames the child won't send, the run loop's
        // `clear_handler_tasks` step will block on it forever – abort
        // here so shutdown still terminates.
        const RUN_DRAIN_GRACE: Duration = Duration::from_millis(500);
        match tokio::time::timeout(RUN_DRAIN_GRACE, run_handle).await {
            Ok(Ok(Ok(()))) => {
                log::info!("bridge run loop drained successfully");
            }
            Ok(Ok(Err(err))) => return Err(err.into()),
            Ok(Err(join_err)) if join_err.is_cancelled() => {
                log::info!("bridge run task was aborted during shutdown");
            }
            Ok(Err(join_err)) => {
                return Err(Error::from(ErrorInner::Custom(
                    eyre::Report::msg(format!(
                        "bridge run task panicked: {join_err}"
                    )),
                )));
            }
            Err(_elapsed) => {
                // The run loop didn't finish in time. Most likely a
                // service handler is wedged – nothing useful to do here
                // but move on; dropping `bridge` below will close the
                // pipes and the child will get EOF.
            }
        }

        // Drop the bridge so the child's stdin pipe is closed. This is
        // what guarantees the child sees EOF on stdin and exits its own
        // bridge run loop, even if the `Close` frame above was aborted
        // before it could be sent.
        drop(bridge);

        // Wait for the child to exit. We use a generous timeout here;
        // if the child doesn't exit on its own (e.g. its shutdown path
        // is blocked) we forcefully kill it.
        let status =
            match tokio::time::timeout(Duration::from_secs(3), child.wait())
                .await
            {
                Ok(result) => result.map_err(ErrorInner::from)?,
                Err(_elapsed) => {
                    log::warn!("child did not exit in time — killing");
                    child.start_kill().ok();
                    child.wait().await.map_err(ErrorInner::from)?
                }
            };
        Ok(status)
    }

    /// Forcibly kill the child process and abort the bridge run loop.
    ///
    /// Returns the child's exit status.
    pub async fn kill(mut self) -> Result<std::process::ExitStatus> {
        self.run_handle.abort();
        self.child.start_kill().map_err(ErrorInner::from)?;
        let status = self.child.wait().await.map_err(ErrorInner::from)?;
        Ok(status)
    }
}

/// Run the binary's `host` mode using the parsed [`HostArgs`].
///
/// This:
///
/// 1. Initializes a stderr-only tracing subscriber at the configured
///    `--log-level` (host stdout is fine for human-facing output).
/// 2. Spawns the child process per the supplied arguments.
/// 3. Optionally pings the child to sanity-check the bridge.
/// 4. Cleanly shuts the bridge down and propagates the child's exit
///    status.
pub async fn run_host(args: HostArgs) -> Result<()> {
    crate::tracing_setup::install_host_tracing(&args.log_level).map_err(
        |e| Error::from(ErrorInner::Custom(eyre::Report::msg(e.to_string()))),
    )?;

    log::info!(
        "starting omni_bridge_test_service host (child={:?}, ping={})",
        args.child_binary,
        args.ping,
    );

    let options = HostSpawnOptions::from(&args);
    let host = Host::spawn(options).await?;

    if args.ping {
        let timeout = args.ping_timeout();
        log::info!("pinging child (timeout={timeout:?})");
        let pong = host.ping(timeout).await?;
        log::info!("ping result: {pong}");
    }

    let status = host.shutdown().await?;
    log::info!("child exited with status {status:?}");

    Ok(())
}
