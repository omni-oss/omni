use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use completion::CompletionCommand;
use config::ConfigCommand;
use env::EnvCommand;
use exec::ExecCommand;
use run::RunCommand;
mod utils;

use crate::{build, commands::hash::HashCommand, tracer::TraceLevel};

pub mod completion;
pub mod config;
pub mod env;
pub mod exec;
pub mod run;
pub mod hash;

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
    #[arg(
        short = 't',
        long = "stdout-trace-level",
        help = "Print traces to stdout",
        value_enum,
        default_value = "info",
        env = "OMNI_STDOUT_TRACE_LEVEL"
    )]
    pub stdout_trace_level: TraceLevel,

    #[arg(
        long,
        group = "stderr-trace",
        help = "Output Error traces to stderr",
        env = "OMNI_STDERR_TRACE_ENABLED",
        default_value = "false",
        action = clap::ArgAction::SetTrue,
        overrides_with = "no_stderr_trace"
    )]
    pub stderr_trace: bool,

    #[arg(
        long,
        group = "stderr-trace",
        help = "Output Error traces to stderr",
        env = "OMNI_STDERR_TRACE_ENABLED",
        default_value = "false",
        action = clap::ArgAction::SetTrue,
        overrides_with = "stderr_trace"
    )]
    pub no_stderr_trace: bool,

    #[arg(
        long = "file-trace-output",
        help = "The file to write traces to",
        default_value = "./.omni/trace/logs"
    )]
    pub file_trace_output: Option<PathBuf>,

    #[arg(
        short = 'f',
        long,
        help = "The trace level to use for file traces",
        value_enum,
        default_value = "none",
        env = "OMNI_FILE_TRACE_LEVEL"
    )]
    pub file_trace_level: TraceLevel,

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

    #[arg(long = "env", help = "The environment to use", env = "OMNI_ENV")]
    pub env: Option<String>,

    #[arg(
        long,
        help = "Inherit environment variables from the parent process", 
        action = clap::ArgAction::SetTrue, 
        default_value_t = false,
        group = "inherit-env-vars",
        conflicts_with = "no_inherit_env_vars",
        overrides_with = "no_inherit_env_vars",
    )]
    pub inherit_env_vars: bool,

    #[arg(
        long,
        help = "Do not inherit environment variables from the parent process",
        action = clap::ArgAction::SetTrue, 
        default_value_t = true,
        group = "inherit-env-vars",
        overrides_with = "inherit_env_vars",
        conflicts_with = "inherit_env_vars",
    )]
    pub no_inherit_env_vars: bool,
}

impl Default for CliArgs {
    fn default() -> Self {
        Self {
            stdout_trace_level: TraceLevel::None,
            stderr_trace: false,
            no_stderr_trace: false,
            file_trace_output: None,
            file_trace_level: TraceLevel::None,
            env_root_dir_marker: None,
            env_file: vec![
                ".env".to_string(),
                ".env.local".to_string(),
                ".env.{ENV}".to_string(),
                ".env.{ENV}.local".to_string(),
            ],
            env: None,
            inherit_env_vars: false,
            no_inherit_env_vars: true,
        }
    }
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
    #[command(about = "Execute specified task in projects")]
    Hash(HashCommand),
}
