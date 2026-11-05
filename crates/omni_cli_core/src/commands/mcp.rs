use std::path::PathBuf;

use clap::Args;
use omni_mcp_core::{OmniMcpServer, serve_stdio};
use omni_tracing_subscriber::TracingSubscriber;
use system_traits::impls::RealSys;
use tracing_futures::WithSubscriber;

use crate::context::{Context, WorkspaceInitConfig};
use omni_tracing_subscriber::TracingConfig;

#[derive(Args, Debug)]
#[command()]
pub struct McpCommand {
    /// Serve multiple workspaces, selected by an absolute working_dir per call.
    #[arg(long, conflicts_with = "root_dir")]
    pub shared: bool,
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
    let server = OmniMcpServer::new(ctx.clone());
    let mut config = ctx.tracing_config().clone();
    // stdout carries the MCP protocol, and there is no stdout trace sink. A
    // trace file that resolves to stdout is rejected at startup (see
    // `reject_stdout_trace_file`). Traces go to stderr at `info` unless the
    // user chose a filter.
    config
        .stderr_filter
        .get_or_insert_with(|| "info".to_string());
    let sub = TracingSubscriber::new(&config, vec![])
        .expect("Failed to create tracing subscriber");
    serve_stdio(server).with_subscriber(sub).await
}

pub async fn run_shared(
    config: WorkspaceInitConfig,
    tracing: &TracingConfig,
) -> eyre::Result<()> {
    let mut tracing = tracing.clone();
    tracing
        .stderr_filter
        .get_or_insert_with(|| "info".to_string());
    let server = OmniMcpServer::new_shared(RealSys, config, tracing.clone());
    let sub = TracingSubscriber::new(&tracing, vec![])?;
    serve_stdio(server).with_subscriber(sub).await
}

/// `true` if `path` is the process's stdout (`/dev/stdout`, `/dev/fd/1`,
/// `/proc/self/fd/1`, or any path naming the same device as the inherited
/// stdout). MCP rejects such a trace file: stdout carries JSON-RPC.
pub fn is_stdout_path(path: &std::path::Path) -> bool {
    let literal = [
        std::path::Path::new("/dev/stdout"),
        std::path::Path::new("/dev/fd/1"),
        std::path::Path::new("/proc/self/fd/1"),
    ];
    if literal.iter().any(|p| path == *p) {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        if let (Ok(a), Ok(b)) =
            (std::fs::metadata(path), std::fs::metadata("/dev/stdout"))
        {
            return a.dev() == b.dev() && a.ino() == b.ino();
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn stdout_aliases_are_detected() {
        for p in ["/dev/stdout", "/dev/fd/1", "/proc/self/fd/1"] {
            assert!(is_stdout_path(Path::new(p)), "{p}");
        }
    }

    #[test]
    fn ordinary_paths_are_not() {
        let dir = std::env::temp_dir().join("omni-trace-test.log");
        assert!(!is_stdout_path(&dir));
        assert!(!is_stdout_path(Path::new("./.omni/trace/omni.log")));
    }
}
