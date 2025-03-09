use clap::Parser;
use commands::{Cli, CliSubcommands};

pub mod build;
pub mod commands;
pub mod context;

fn set_tracing_level(level: u8) -> eyre::Result<()> {
    let level = match level {
        1 => tracing::Level::ERROR,
        2 => tracing::Level::WARN,
        3 => tracing::Level::INFO,
        4 => tracing::Level::DEBUG,
        5.. => tracing::Level::TRACE,
        0 => return Ok(()),
    };

    tracing_subscriber::fmt().with_max_level(level).init();

    Ok(())
}

#[tokio::main(flavor = "multi_thread")]
pub async fn main() -> eyre::Result<()> {
    let cli = Cli::parse();

    set_tracing_level(cli.args.verbose + 1)?;

    let context = context::build(&cli.args)?;

    match cli.subcommand {
        CliSubcommands::Exec(ref exec) => {
            commands::exec::run(exec, &context).await?;
        }
        CliSubcommands::Env(ref env) => {
            commands::env::run(env, &context).await?;
        }
    }

    Ok(())
}
