//! Test-only support crate that bridges to itself over stdio.
//!
//! This crate is intentionally light on functionality – it exists to:
//!
//! 1. Provide a small clap-based binary that can run either as a *host* (a
//!    parent that spawns a child process and drives `bridge_rpc` over the
//!    child's stdin/stdout) or as a *client* (a child that registers every
//!    service from [`bridge_rpc_services`] against a router and serves them
//!    over its own stdin/stdout).
//! 2. Expose the [`Host`] helper so other crates can write integration
//!    tests against the bridge stack without re-implementing the
//!    spawn/transport plumbing.
//!
//! # Usage from another test crate
//!
//! ```no_run
//! use std::time::Duration;
//! use omni_bridge_test_service::{Host, HostSpawnOptions};
//!
//! # async fn demo() -> omni_bridge_test_service::Result<()> {
//! // Spawns the current executable in `client` mode by default. Pass
//! // `with_child_binary` if you want to point at a different binary.
//! let host = Host::spawn(HostSpawnOptions::default()).await?;
//!
//! // Use the underlying client handle to make RPC calls...
//! let _client = host.client_handle();
//!
//! // Or just check the bridge is alive:
//! assert!(host.ping(Duration::from_secs(1)).await?);
//!
//! host.shutdown().await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Two CLI modes
//!
//! ```text
//! # Run a child in the background and ping it once.
//! omni_bridge_test_service host --ping
//!
//! # Behave as the bridge server. Reads frames from stdin, writes to stdout.
//! # **No logging** is emitted in this mode because stdout is the transport.
//! omni_bridge_test_service client
//! ```

pub mod cli;
pub mod client;
pub mod error;
pub mod host;
pub mod tracing_setup;
// @anchor:mods

pub use cli::{Cli, ClientArgs, HostArgs, Mode};
pub use client::{
    StdioTransport, build_default_router, build_stdio_bridge, run_client,
};
pub use error::{Error, ErrorKind, Result};
pub use host::{Host, HostSpawnOptions, HostTransport, run_host};
// @anchor:uses

use clap::Parser as _;

/// Run the binary with the supplied [`Cli`], dispatching to either
/// [`run_host`] or [`run_client`] depending on the mode.
pub async fn run(cli: Cli) -> Result<()> {
    match cli.mode {
        Mode::Host(args) => run_host(args).await,
        Mode::Client(args) => run_client(args).await,
    }
}

/// Convenience entry point for `main.rs`. Parses CLI args, builds a
/// multi-threaded Tokio runtime, and dispatches to [`run`].
pub fn main_entry() -> Result<()> {
    let cli = Cli::parse();

    // Build the runtime explicitly so we don't pull `tokio::main` macros
    // into a `no_main` situation.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| {
            Error::from(error::ErrorInner::Custom(eyre::Report::msg(
                format!("failed to build tokio runtime: {e}"),
            )))
        })?;

    runtime.block_on(run(cli))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_parses_host_subcommand() {
        let cli = Cli::try_parse_from(["omni_bridge_test_service", "host"])
            .expect("should parse");
        match cli.mode {
            Mode::Host(_) => {}
            other => panic!("expected Mode::Host, got {other:?}"),
        }
    }

    #[test]
    fn cli_parses_client_subcommand() {
        let cli = Cli::try_parse_from(["omni_bridge_test_service", "client"])
            .expect("should parse");
        match cli.mode {
            Mode::Client(args) => {
                assert_eq!(args.fs_prefix, cli::DEFAULT_FS_PREFIX);
                assert_eq!(args.proc_prefix, cli::DEFAULT_PROC_PREFIX);
                assert_eq!(args.log_path, cli::DEFAULT_LOG_PATH);
            }
            other => panic!("expected Mode::Client, got {other:?}"),
        }
    }

    #[test]
    fn host_args_can_be_converted_to_spawn_options() {
        let cli = Cli::try_parse_from([
            "omni_bridge_test_service",
            "host",
            "--child-arg",
            "--foo",
            "--child-arg",
            "bar",
            "--no-client-subcommand",
            "--warmup-ms",
            "50",
            "--ping",
        ])
        .expect("should parse");

        let args = match cli.mode {
            Mode::Host(args) => args,
            other => panic!("expected Mode::Host, got {other:?}"),
        };

        assert!(args.ping);
        assert!(args.no_client_subcommand);
        assert_eq!(args.child_args, vec!["--foo", "bar"]);
        assert_eq!(args.warmup_ms, 50);

        let opts = HostSpawnOptions::from(&args);
        assert!(opts.skip_client_subcommand);
        assert_eq!(opts.child_args, vec!["--foo", "bar"]);
        assert_eq!(
            opts.warmup,
            Some(std::time::Duration::from_millis(50)),
        );
    }
}
