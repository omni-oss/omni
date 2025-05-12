#![allow(dead_code)]

use clap::Parser;
use commands::{Cli, CliSubcommands};

mod build;
mod commands;
mod configurations;
mod constants;
mod context;
mod core;

fn set_tracing_level(level: u8) -> eyre::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(match level {
            1 => tracing::Level::ERROR,
            2 => tracing::Level::WARN,
            3 => tracing::Level::INFO,
            4 => tracing::Level::DEBUG,
            5.. => tracing::Level::TRACE,
            0 => return Ok(()),
        })
        .init();

    Ok(())
}

#[tokio::main(flavor = "multi_thread")]
pub async fn main() -> eyre::Result<()> {
    let cli = Cli::parse();

    set_tracing_level(cli.args.verbose + 1)?;

    let mut context = context::build(&cli.args)?;

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
