use clap::{Args, Parser, Subcommand};
use completion::CompletionCommand;
use config::ConfigCommand;
use env::EnvCommand;
use exec::ExecCommand;
use run::RunCommand;

use crate::build;

pub mod completion;
pub mod config;
pub mod env;
pub mod exec;
pub mod run;

const ABOUT: &str = "omni is development workflow orchestration tool";
const LONG_ABOUT: &str = r#"
Flexible task runner and scaffolding CLI for streamlined development workflows.
"#;

#[derive(Parser)]
#[command(author = "Clarence Manuel <rencedm112@gmail.com>")]
#[command(version = build::PKG_VERSION, propagate_version = true)]
#[command(about = ABOUT)]
#[command(long_about = LONG_ABOUT)]
#[command(next_line_help = true)]
pub struct Cli {
    #[command(flatten)]
    pub args: CliArgs,

    #[command(subcommand)]
    pub subcommand: CliSubcommands,
}

#[derive(Args)]
#[command()]
pub struct CliArgs {
    #[arg(short = 'v', long = "verbose", help = "Show trace level", action = clap::ArgAction::Count, default_value_t = 0)]
    pub verbose: u8,

    #[arg(
        short = 'r',
        long,
        help = "The file which marks the root dir where to stop looking for env files",
        default_value = "workspace.omni.yaml"
    )]
    pub env_root_dir_marker: Option<String>,

    #[arg(
        short = 'e',
        long,
        help = "The env files to load per directory",
        default_values = [
            ".env",
            ".env.local",
            ".env.{ENV}",
            ".env.{ENV}.local",
        ],
        action = clap::ArgAction::Append,
    )]
    pub env_file: Vec<String>,

    #[arg(long = "env", help = "The environment to use", env = "ENV")]
    pub env: Option<String>,
}

#[derive(Subcommand)]
#[command(rename_all = "kebab-case", about = "")]
pub enum CliSubcommands {
    #[command(about = "Output environment variabls values")]
    Env(EnvCommand),
    #[command(about = "Execute an ad-hoc command in projects")]
    Exec(ExecCommand),
    #[command(about = "Print configuration schemas in JSON")]
    Config(ConfigCommand),
    #[command(about = "Print shell completions")]
    Completion(CompletionCommand),
    #[command(about = "Execute specified task in projects")]
    Run(RunCommand),
}
