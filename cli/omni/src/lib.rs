#![feature(try_blocks)]

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
use system_traits::impls::RealSys;

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

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
            let mut context = create_ctx()?;
            commands::env::run(env, &mut context).await?;
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
    }

    Ok(())
}

#[tokio::main(flavor = "multi_thread")]
#[cfg_attr(feature = "enable-tracing", tracing::instrument(err))]
pub async fn main() -> eyre::Result<()> {
    color_eyre::config::HookBuilder::default()
        .display_location_section(cfg!(debug_assertions))
        .install()?;

    let cli = Cli::parse();

    let ws_root_dir = get_root_dir(&RealSys).ok();
    let trace_file_path = cli
        .args
        .file_trace_output
        .clone()
        .or_else(|| Some(PathBuf::from("./omni/trace/logs")));

    let trace_file_path = if let Some(path) = trace_file_path {
        Some(
            if !path.has_root()
                && let Some(ref root) = ws_root_dir
            {
                root.join(path)
            } else {
                path
            },
        )
    } else {
        None
    };

    let tracing_config = TracingConfig {
        stderr_trace_enabled: cli.args.stderr_trace,
        file_path: trace_file_path,
        file_trace_level: cli.args.file_trace_level.value(),
        stdout_trace_level: cli.args.stdout_trace_level.value(),
    };

    #[cfg(feature = "enable-tracing")]
    {
        init_tracing(&tracing_config)?;
        trace::trace!(?tracing_config, "tracing_initialized");
    }

    run(
        &cli.subcommand,
        &cli.args,
        &tracing_config,
        ws_root_dir.as_deref(),
    )
    .await?;

    Ok(())
}
