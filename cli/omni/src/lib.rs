use clap::Parser as _;
#[cfg(feature = "enable-tracing")]
use omni_cli_core::tracer::TracerConfig;
use omni_cli_core::{
    commands::{self, Cli, CliSubcommands},
    context,
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

pub async fn run(cli: Cli) -> eyre::Result<()> {
    let sys = RealSys;
    let mut context =
        context::Context::from_args_and_sys(&cli.args, sys.clone())?;

    match cli.subcommand {
        CliSubcommands::Exec(ref exec) => {
            commands::exec::run(exec, &context).await?;
        }
        CliSubcommands::Env(ref env) => {
            commands::env::run(env, &mut context).await?;
        }
        CliSubcommands::Config(ref config) => {
            commands::config::run(config, &context).await?;
        }
        CliSubcommands::Completion(ref completion) => {
            commands::completion::run(completion, &context).await?;
        }
        CliSubcommands::Run(ref run) => {
            commands::run::run(run, &context).await?;
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
        trace::debug!("Tracing config: {:?}", tracing_config);
    }

    run(cli).await
}
