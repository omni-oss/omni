use std::path::PathBuf;

use clap::Args;
use omni_mcp_core::{OmniMcpServer, serve_stdio};
use omni_messages::NoopSubscriber;
use omni_tracing_subscriber::{Level, TracingSubscriber};
use system_traits::impls::RealSys;
use tracing_futures::WithSubscriber;

use crate::context::Context;

#[derive(Args, Debug)]
#[command()]
pub struct McpCommand {
    /// Workspace root directory. When omitted, omni walks up from the
    /// current working directory to find an `workspace.omni.yaml` file.
    /// MCP clients should set this to the workspace root so the server
    /// works correctly regardless of the client's own working directory.
    /// It also sets the current dir of the process.
    /// Can also be set via the `OMNI_ROOT_DIR` environment variable.
    #[arg(long, value_name = "DIR", env = "OMNI_ROOT_DIR")]
    pub root_dir: Option<PathBuf>,
}

pub async fn run(cmd: &McpCommand, ctx: &Context<RealSys>) -> eyre::Result<()> {
    if let Some(root) = &cmd.root_dir {
        std::env::set_current_dir(root)?;
    }
    let server = OmniMcpServer::new(ctx.clone(), NoopSubscriber);
    let mut config = ctx.tracing_config().clone();
    config.stdout_level = Level::Off;
    // stdio logging is used for the MCP protocol, so we don't want to log anything to stdout.
    // We do want to log to stderr, so we set the stderr level to Info.
    config.stderr_level = Level::Info;
    let sub = TracingSubscriber::new(&config, vec![])
        .expect("Failed to create tracing subscriber");
    serve_stdio(server).with_subscriber(sub).await
}
