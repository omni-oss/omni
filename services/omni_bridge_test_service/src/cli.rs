//! Command-line interface for the bridge test service binary.
//!
//! The binary supports two top-level modes:
//!
//! - [`Mode::Host`] – spawns a child process running the same binary in
//!   `client` mode and drives RPC requests over its stdin/stdout. Useful for
//!   smoke-testing the wiring end-to-end from a single binary, and as a
//!   reference for higher-level integration tests written against the
//!   [`Host`](crate::host::Host) helper.
//! - [`Mode::Client`] – runs the bridge with all
//!   [`bridge_rpc_services`](::bridge_rpc_services) registered, using the
//!   process's stdin/stdout as the transport. **No logging/tracing is
//!   initialized in this mode** because stdout is reserved for RPC frames.

use std::path::PathBuf;
use std::time::Duration;

use clap::{Args, Parser, Subcommand};

/// Default prefix used for the FS service routes when not overridden via
/// CLI flags.
pub const DEFAULT_FS_PREFIX: &str = bridge_rpc_services::DEFAULT_FS_PREFIX;
/// Default prefix used for the proc service routes when not overridden via
/// CLI flags.
pub const DEFAULT_PROC_PREFIX: &str = bridge_rpc_services::DEFAULT_PROC_PREFIX;
/// Default path used for the log service when not overridden via CLI flags.
pub const DEFAULT_LOG_PATH: &str = bridge_rpc_services::DEFAULT_LOG_PATH;

/// Top-level CLI parser for the binary.
#[derive(Parser, Debug)]
#[command(
    name = "omni_bridge_test_service",
    about = "Test service that exercises bridge_rpc_services over a stdio bridge",
    long_about = "Runs a bridge_rpc test service in either `host` or `client` mode. \n\n\
    In `host` mode, the binary spawns a child process (defaulting to the current \
    executable invoked with `client`) and connects to it via the child's stdin/stdout. \n\n\
    In `client` mode, the bridge is run over the process's own stdin/stdout, with \
    every service from `bridge_rpc_services` registered against the router. \
    Logging and tracing are intentionally NOT initialized in this mode since stdout \
    is reserved for RPC frames."
)]
pub struct Cli {
    /// Selected execution mode.
    #[command(subcommand)]
    pub mode: Mode,
}

/// Top-level subcommands.
#[derive(Subcommand, Debug, Clone)]
pub enum Mode {
    /// Spawn a child process and drive bridge requests against it.
    #[command(about = "Spawn and drive a child `client` process")]
    Host(HostArgs),

    /// Run the bridge as a stdin/stdout RPC server.
    #[command(about = "Run as the stdio-driven RPC server (no stdout logging)")]
    Client(ClientArgs),
}

/// Arguments for the `host` subcommand.
#[derive(Args, Debug, Clone)]
pub struct HostArgs {
    /// Path to the child binary to spawn. Defaults to the currently-running
    /// executable so that running the host without arguments will spawn
    /// itself in client mode.
    #[arg(
        long,
        env = "OMNI_BRIDGE_TEST_CHILD_BINARY",
        help = "Path to the child binary (defaults to the current executable)"
    )]
    pub child_binary: Option<PathBuf>,

    /// Extra arguments appended to the child invocation, before the
    /// `client` subcommand. Useful when the child binary requires
    /// additional options to reach `client` mode.
    #[arg(
        long = "child-arg",
        value_name = "ARG",
        allow_hyphen_values = true,
        help = "Extra argument forwarded to the child process (repeatable)"
    )]
    pub child_args: Vec<String>,

    /// When true, the child is invoked as `<child-binary> <child-args>...`
    /// without injecting the `client` subcommand. Useful when the child
    /// binary exposes the bridge directly without subcommand dispatch.
    #[arg(
        long,
        default_value_t = false,
        help = "Skip injecting the `client` subcommand into the child invocation"
    )]
    pub no_client_subcommand: bool,

    /// Number of milliseconds to wait for the bridge to be ready before
    /// running the requested actions.
    #[arg(
        long,
        default_value_t = 0,
        value_name = "MS",
        help = "Time to wait after spawn before running actions"
    )]
    pub warmup_ms: u64,

    /// Number of milliseconds for the ping timeout in the smoke-test action.
    #[arg(
        long,
        default_value_t = 1_000,
        value_name = "MS",
        help = "Ping timeout when running the `--ping` smoke test"
    )]
    pub ping_timeout_ms: u64,

    /// Run a ping smoke-test after spawning the child.
    #[arg(
        long,
        default_value_t = false,
        help = "Send a ping to the child once the bridge is up"
    )]
    pub ping: bool,

    /// Stderr log level for the host process.
    #[arg(
        long,
        default_value = "info",
        value_name = "LEVEL",
        help = "Stderr log level for the host (off, error, warn, info, debug, trace)"
    )]
    pub log_level: String,
}

impl HostArgs {
    /// Convert the configured ping timeout into a [`Duration`].
    pub fn ping_timeout(&self) -> Duration {
        Duration::from_millis(self.ping_timeout_ms)
    }

    /// Convert the configured warmup duration into a [`Duration`].
    pub fn warmup(&self) -> Duration {
        Duration::from_millis(self.warmup_ms)
    }
}

/// Arguments for the `client` subcommand.
#[derive(Args, Debug, Clone)]
pub struct ClientArgs {
    /// Path prefix used for FS routes (e.g. `/fs`).
    #[arg(
        long,
        default_value = DEFAULT_FS_PREFIX,
        value_name = "PREFIX",
        help = "Prefix prepended to FS routes",
    )]
    pub fs_prefix: String,

    /// Path prefix used for proc routes (e.g. `/proc`).
    #[arg(
        long,
        default_value = DEFAULT_PROC_PREFIX,
        value_name = "PREFIX",
        help = "Prefix prepended to proc routes",
    )]
    pub proc_prefix: String,

    /// Path used for the log service (e.g. `/log`).
    #[arg(
        long,
        default_value = DEFAULT_LOG_PATH,
        value_name = "PATH",
        help = "Path of the log service",
    )]
    pub log_path: String,
}

impl Default for ClientArgs {
    fn default() -> Self {
        Self {
            fs_prefix: DEFAULT_FS_PREFIX.to_string(),
            proc_prefix: DEFAULT_PROC_PREFIX.to_string(),
            log_path: DEFAULT_LOG_PATH.to_string(),
        }
    }
}

impl ClientArgs {
    /// Convert to a [`bridge_rpc_services::RegisterServicesOptions`] using
    /// the configured prefixes/paths.
    pub fn to_register_options(
        &self,
    ) -> bridge_rpc_services::RegisterServicesOptions {
        bridge_rpc_services::RegisterServicesOptions {
            fs_prefix: self.fs_prefix.clone(),
            proc_prefix: self.proc_prefix.clone(),
            log_path: self.log_path.clone(),
        }
    }
}
