#![allow(dead_code)]

#[cfg(feature = "enable-tracing")]
use std::path::PathBuf;

use clap::Parser;
use commands::{Cli, CliSubcommands};
#[cfg(feature = "enable-tracing")]
use system_traits::impls::RealSys as RealSysSync;

#[cfg(feature = "enable-tracing")]
use crate::tracer::TracerConfig;

mod build;
mod commands;
mod configurations;
mod constants;
mod context;
mod core;
mod tracer;
mod utils;

#[cfg(feature = "enable-tracing")]
fn init_tracing(config: &TracerConfig) -> eyre::Result<()> {
    use tracing_subscriber::util::SubscriberInitExt;

    use crate::tracer::TracerSubscriber;

    TracerSubscriber::new(config)?.try_init()?;

    Ok(())
}

#[tokio::main(flavor = "multi_thread")]
pub async fn main() -> eyre::Result<()> {
    let cli = Cli::parse();

    #[cfg(feature = "enable-tracing")]
    {
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

    let sys = RealSysSync;
    let mut context =
        context::Context::from_args_and_sys(&cli.args, sys.clone())?;

    match cli.subcommand {
        CliSubcommands::Exec(ref exec) => {
            commands::exec::run(exec, &mut context).await?;
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
            commands::run::run(run, &mut context).await?;
        }
    }

    Ok(())
}
