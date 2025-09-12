use std::process::ExitCode;

use clap::Parser as _;
#[cfg(feature = "enable-tracing")]
use omni_cli_core::tracer::TracerConfig;
use omni_cli_core::{
    commands::{self, Cli, CliArgs, CliSubcommands},
    context::{self, Context, ContextError},
};
use system_traits::impls::RealSys;

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[cfg(feature = "enable-tracing")]
fn init_tracing(config: &TracerConfig) -> eyre::Result<()> {
    use tracing_subscriber::util::SubscriberInitExt;

    use omni_cli_core::tracer::TracerSubscriber;

    TracerSubscriber::new(config)?.try_init()?;

    Ok(())
}

#[inline(always)]
fn exit(code: ExitCode) -> ! {
    std::process::exit(if code == ExitCode::SUCCESS { 0 } else { 1 })
}

#[inline(always)]
fn ctx(args: &CliArgs) -> Result<Context<RealSys>, ContextError> {
    let sys = RealSys;
    context::from_args_and_sys(args, sys)
}

pub async fn run(sc: &CliSubcommands, args: &CliArgs) -> eyre::Result<()> {
    match sc {
        CliSubcommands::Config(config) => {
            commands::config::run(config).await?;
        }
        CliSubcommands::Completion(completion) => {
            commands::completion::run(completion).await?;
        }
        CliSubcommands::Exec(exec) => {
            let context = ctx(args)?;
            let res = commands::exec::run(exec, &context).await?;
            exit(res);
        }
        CliSubcommands::Env(env) => {
            let mut context = ctx(args)?;
            commands::env::run(env, &mut context).await?;
        }
        CliSubcommands::Run(run) => {
            let context = ctx(args)?;
            let res = commands::run::run(run, &context).await?;
            exit(res);
        }
        CliSubcommands::Hash(hash_command) => {
            let context = ctx(args)?;
            commands::hash::run(hash_command, &context).await?;
        }
        CliSubcommands::Declspec(declspec_command) => {
            commands::declspec::run(declspec_command).await?;
        }
        CliSubcommands::Cache(cache_command) => {
            let context = ctx(args)?;
            commands::cache::run(cache_command, &context).await?;
        }
    }

    Ok(())
}

#[tokio::main(flavor = "multi_thread")]
pub async fn main() -> eyre::Result<()> {
    let cli = Cli::parse();

    #[cfg(feature = "enable-tracing")]
    {
        use std::path::PathBuf;

        use omni_cli_core::tracer::TracerConfig;

        let tracing_config = TracerConfig {
            stderr_trace_enabled: cli.args.stderr_trace,
            file_path: cli
                .args
                .file_trace_output
                .clone()
                .or_else(|| Some(PathBuf::from("./omni/trace/logs"))),
            file_trace_level: cli.args.file_trace_level,
            stdout_trace_level: cli.args.stdout_trace_level,
        };
        init_tracing(&tracing_config)?;
        trace::trace!("Tracing config: {:?}", tracing_config);
    }

    run(&cli.subcommand, &cli.args).await
}
