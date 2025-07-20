#![allow(dead_code)]

use clap::Parser;
use commands::{Cli, CliSubcommands};
use js_runtime::{JsRuntime as _, impls::DelegatingJsRuntime};
use system_traits::impls::RealSys as RealSysSync;
use tracing_subscriber::fmt;

mod build;
mod commands;
mod configurations;
mod constants;
mod context;
mod core;
mod utils;

fn init_tracing(level: u8) -> eyre::Result<()> {
    let format = fmt::format()
        .with_file(false)
        .with_level(false)
        .with_line_number(false)
        .with_thread_ids(false)
        .with_thread_names(false)
        .without_time()
        .with_target(false)
        .with_source_location(false);

    tracing_subscriber::fmt()
        .event_format(format)
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

const TEST_SCRIPT: &str = r#"
async function main(arg) {
    console.log("Hello, World!");
}

type Script = typeof main;

await main();
"#;

#[tokio::main(flavor = "multi_thread")]
pub async fn main() -> eyre::Result<()> {
    let cli = Cli::parse();

    init_tracing(cli.args.verbose + 3)?;

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
        CliSubcommands::JsTest => {
            let js = context.get_workspace_configuration().scripting.js;

            let mut rt =
                DelegatingJsRuntime::new(sys.clone(), js.runtime.into());
            rt.run(TEST_SCRIPT, Some(context.root_dir())).await?;
        }
    }

    Ok(())
}
