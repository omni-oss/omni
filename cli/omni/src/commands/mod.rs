use clap::{Args, Parser, Subcommand};
use env::EnvCommand;
use exec::ExecCommand;

use crate::build;

pub mod env;
pub mod exec;

const ABOUT: &str = "omni is a build system and a monorepo management tool";
const LONG_ABOUT: &str = r#"
omni is a build system and a monorepo management tool
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
pub struct CliArgs {
    #[arg(short = 'v', long = "verbose", help = "Show trace level", action = clap::ArgAction::Count, default_value_t = 0)]
    pub verbose: u8,

    #[arg(
        short,
        long,
        help = "The path to the omni config file",
        default_value = "omni.toml"
    )]
    pub config: Option<String>,
}

#[derive(Subcommand)]
pub enum CliSubcommands {
    Env(#[command(flatten)] EnvCommand),
    Exec(#[command(flatten)] ExecCommand),
}
