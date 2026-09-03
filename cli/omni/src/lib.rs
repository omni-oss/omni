use std::{
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::Parser as _;
use omni_cli_core::{
    commands::{self, Cli, CliArgs, CliSubcommands},
    context::{self, Context, ContextError, get_root_dir},
};
use omni_tracing_subscriber::TracingConfig;
use scopeguard::defer;
use system_traits::impls::RealSys;
use trace::Level;

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[cfg(all(not(feature = "dhat-heap"), not(target_os = "windows")))]
#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[cfg(feature = "enable-tracing")]
fn init_tracing(
    config: &omni_tracing_subscriber::TracingConfig,
) -> eyre::Result<()> {
    use omni_tracing_subscriber::TracingSubscriber;
    use tracing_subscriber::util::SubscriberInitExt;

    TracingSubscriber::new(config, vec![])?.try_init()?;

    Ok(())
}

#[inline(always)]
fn exit(code: ExitCode) -> ! {
    std::process::exit(if code == ExitCode::SUCCESS { 0 } else { 1 })
}

#[inline(always)]
#[allow(clippy::result_large_err)]
fn ctx(
    args: &CliArgs,
    tracing: &TracingConfig,
    ws_root_dir: Option<&Path>,
) -> Result<Context<RealSys>, ContextError> {
    trace::trace!(?args, "cli_args_received");

    let sys = RealSys;
    if let Some(root) = ws_root_dir {
        context::from_args_root_dir_and_sys(args, root, sys, tracing)
    } else {
        context::from_args_and_sys(args, sys, tracing)
    }
}

pub async fn run(
    sc: &CliSubcommands,
    args: &CliArgs,
    tracing: &TracingConfig,
    ws_root_dir: Option<&Path>,
) -> eyre::Result<()> {
    #[allow(clippy::result_large_err)]
    let create_ctx = || ctx(args, tracing, ws_root_dir);

    match sc {
        CliSubcommands::Config(config) => {
            commands::config::run(config).await?;
        }
        CliSubcommands::Completion(completion) => {
            commands::completion::run(completion).await?;
        }
        CliSubcommands::Exec(exec) => {
            let context = create_ctx()?;
            let res = commands::exec::run(exec, &context).await?;
            exit(res);
        }
        CliSubcommands::Env(env) => {
            let context = create_ctx()?;
            commands::env::run(env, &context).await?;
        }
        CliSubcommands::Run(run) => {
            let context = create_ctx()?;
            let res = commands::run::run(run, &context).await?;
            exit(res);
        }
        CliSubcommands::Hash(hash_command) => {
            let context = create_ctx()?;
            commands::hash::run(hash_command, &context).await?;
        }
        CliSubcommands::Declspec(declspec_command) => {
            commands::declspec::run(declspec_command).await?;
        }
        CliSubcommands::Cache(cache_command) => {
            let context = create_ctx()?;
            commands::cache::run(cache_command, &context).await?;
        }
        CliSubcommands::Generator(command) => {
            let context = create_ctx()?;
            commands::generator::run(command, &context).await?;
        }
        CliSubcommands::Init(command) => {
            commands::init::run(command).await?;
        }
        CliSubcommands::Project(cmd) => {
            let context = create_ctx()?;
            commands::project::run(cmd, &context).await?;
        }
        CliSubcommands::Projection(cmd) => {
            let context = create_ctx()?;
            commands::projection::run(cmd, &context).await?;
        }
        CliSubcommands::Tool(cmd) => {
            let context = create_ctx()?;
            commands::tool::run(cmd, &context).await?;
        }
        CliSubcommands::Mcp(mcp) => {
            let context = if let Some(root) = &mcp.root_dir {
                context::from_args_root_dir_and_sys(
                    args, root, RealSys, tracing,
                )?
            } else {
                create_ctx()?
            };
            commands::mcp::run(mcp, &context).await?;
        }
    }

    Ok(())
}

#[cfg_attr(feature = "enable-tracing", tracing::instrument(level = Level::DEBUG, err))]
async fn run_main() -> eyre::Result<()> {
    color_eyre::config::HookBuilder::default()
        .display_location_section(cfg!(debug_assertions))
        .install()?;

    let cli = Cli::parse();

    let ws_root_dir = if cli.subcommand.is_mcp() {
        None
    } else {
        get_root_dir(&RealSys).ok()
    };

    let trace_file_path = cli
        .args
        .file_trace_output
        .clone()
        .or_else(|| Some(PathBuf::from("./.omni/trace/omni.log")));

    let trace_file_path = trace_file_path.map(|path| {
        if !path.has_root()
            && let Some(ref root) = ws_root_dir
        {
            root.join(path)
        } else {
            path
        }
    });

    let tracing_config = TracingConfig {
        file_path: trace_file_path,
        file_level: cli.args.file_trace_level.value(),
        stdout_level: cli.args.stdout_log_level.value(),
        stdout_show_traces: cli.args.stdout_show_traces,
        stderr_level: cli.args.stderr_log_level.value(),
        stderr_show_traces: cli.args.stderr_show_traces,
    };

    #[cfg(feature = "enable-tracing")]
    {
        init_tracing(&tracing_config)?;
        trace::trace!(?tracing_config, "tracing_initialized");
    }

    omni_setup::initialize(omni_setup::InitConfig::builder().build())?;
    defer! {
        let res = omni_setup::deinitialize();

        if let Err(error) = res {
            trace::error!(%error, "deinit_failed");
        }
    };

    run(
        &cli.subcommand,
        &cli.args,
        &tracing_config,
        ws_root_dir.as_deref(),
    )
    .await?;

    Ok(())
}

// The generator pipeline awaits its `run-generator` recursion on the thread
// that calls `block_on`, and each nested level keeps a suspended async frame
// live. Debug builds make those frames large, and the Windows process main
// thread has only about 1 MiB of stack, so a few levels of nesting overflow it.
// Run the runtime on a thread with a generous stack (and give tokio's own
// workers the same) so nesting depth is bounded by `max_depth`, not the OS.
const RUNTIME_STACK_SIZE: usize = 32 * 1024 * 1024;

fn run_runtime() -> eyre::Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .thread_stack_size(RUNTIME_STACK_SIZE)
        .enable_all()
        .build()?;
    let result = rt.block_on(run_main());
    // micro-optimization to force blocking background tasks to be abondoned
    // instead of waiting for them to finish
    rt.shutdown_background();
    result
}

pub fn main() -> eyre::Result<()> {
    let worker = std::thread::Builder::new()
        .name("omni-main".to_string())
        .stack_size(RUNTIME_STACK_SIZE)
        .spawn(run_runtime)?;

    match worker.join() {
        Ok(result) => result,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}
