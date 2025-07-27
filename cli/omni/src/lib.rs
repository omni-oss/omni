#![allow(dead_code)]

use clap::Parser;
use commands::{Cli, CliSubcommands};
use system_traits::impls::RealSys as RealSysSync;

mod build;
mod commands;
mod configurations;
mod constants;
mod context;
mod core;
mod utils;

#[cfg(feature = "enable-tracing")]
fn init_tracing(level: u8) -> eyre::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(match level {
            1 => trace::Level::ERROR,
            2 => trace::Level::WARN,
            3 => trace::Level::INFO,
            4 => trace::Level::DEBUG,
            5.. => trace::Level::TRACE,
            0 => return Ok(()),
        })
        .init();

    Ok(())
}

fn init_logging() -> eyre::Result<()> {
    env_logger::init_from_env("OMNI_LOG");

    Ok(())
}

#[tokio::main(flavor = "multi_thread")]
pub async fn main() -> eyre::Result<()> {
    let cli = Cli::parse();

    #[cfg(feature = "enable-tracing")]
    init_tracing(cli.args.trace_level)?;

    init_logging()?;

    let sys = RealSysSync;
    let mut context =
        context::Context::from_args_and_sys(&cli.args, sys.clone())?;

    match cli.subcommand {
        CliSubcommands::Exec(ref exec) => {
            commands::exec::run(exec, &mut context).await?;
        }
        CliSubcommands::Env(ref env) => {
            commands::env::run(env, &context).await?;
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
