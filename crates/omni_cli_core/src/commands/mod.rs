use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use clap_utils::EnumValueAdapter;
use completion::CompletionCommand;
use config::ConfigCommand;
use env::EnvCommand;
use exec::ExecCommand;
use mcp::McpCommand;
use omni_tracing_subscriber::Level;
use run::RunCommand;
use strum::{EnumDiscriminants, EnumIs};
mod common_args;
mod common_types;
mod generator_common_args;
mod parser;
mod utils;

use crate::{
    build,
    commands::{
        cache::CacheCommand, declspec::DeclspecCommand,
        generator::GeneratorCommand, hash::HashCommand, init::InitCommand,
        project::ProjectCommand,
    },
};

pub mod cache;
pub mod completion;
pub mod config;
pub mod declspec;
pub mod env;
pub mod exec;
pub mod generator;
mod generator_utils;
pub mod hash;
pub mod init;
pub mod mcp;
pub mod project;
pub mod run;

const ABOUT: &str = "omni is development workflow orchestration tool";
const LONG_ABOUT: &str = r#"
Flexible task runner and scaffolding CLI for streamlined development workflows.
"#;

#[derive(Parser)]
#[command(author = "Clarence Manuel <rencedm112@gmail.com>")]
#[command(version = build::PKG_VERSION, propagate_version = true)]
#[command(name = "omni")]
#[command(about = ABOUT)]
#[command(long_about = LONG_ABOUT)]
#[command(next_line_help = true)]
pub struct Cli {
    #[command(flatten)]
    pub args: CliArgs,

    #[command(subcommand)]
    pub subcommand: CliSubcommands,
}

#[derive(Args, Debug)]
#[command()]
pub struct CliArgs {
    #[arg(
        short = 'l',
        long = "stdout-logs-level",
        help = "Max level of logs to stdout",
        value_enum,
        default_value = "info",
        env = "OMNI_STDOUT_LOG_LEVEL"
    )]
    pub stdout_log_level: EnumValueAdapter<Level>,

    #[arg(
        short = 't',
        long = "stdout-show-traces",
        help = "Include traces to stdout",
        default_value_t = false,
        action = clap::ArgAction::SetTrue,
        env = "OMNI_STDOUT_SHOW_TRACES"
    )]
    pub stdout_show_traces: bool,

    #[arg(
        long,
        group = "stderr-log",
        help = "Output Error log to stderr",
        env = "OMNI_STDERR_LOG_ENABLED",
        default_value_t = false,
        action = clap::ArgAction::SetTrue,
    )]
    pub stderr_log: bool,

    #[arg(
        long = "stderr-show-traces",
        help = "Include error traces to stderr",
        default_value_t = false,
        action = clap::ArgAction::SetTrue,
        env = "OMNI_STDERR_SHOW_TRACES"
    )]
    pub stderr_show_traces: bool,

    #[arg(
        long = "file-trace-output",
        help = "The file to write traces to",
        default_value = "./.omni/trace/omni.log"
    )]
    pub file_trace_output: Option<PathBuf>,

    #[arg(
        short = 'f',
        long,
        help = "The trace level to use for file traces",
        value_enum,
        default_value = "off",
        env = "OMNI_FILE_TRACE_LEVEL"
    )]
    pub file_trace_level: EnumValueAdapter<Level>,

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
        action = clap::ArgAction::Append,
    )]
    pub env_file: Option<Vec<String>>,

    #[arg(long = "env", help = "The environment to use", env = "OMNI_ENV")]
    pub env: Option<String>,

    #[arg(
        long,
        short = 'i',
        help = "Inherit environment variables from the parent process",
        action = clap::ArgAction::SetTrue,
        default_value_t = false,
        group = "inherit-env-vars",
    )]
    pub inherit_env_vars: bool,
}

impl Default for CliArgs {
    fn default() -> Self {
        Self {
            stdout_log_level: EnumValueAdapter::new(Level::Off),
            stderr_log: false,
            file_trace_output: None,
            file_trace_level: EnumValueAdapter::new(Level::Off),
            env_root_dir_marker: None,
            env_file: None,
            env: None,
            inherit_env_vars: false,
            stdout_show_traces: false,
            stderr_show_traces: false,
        }
    }
}

#[derive(Subcommand, EnumIs, EnumDiscriminants)]
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

    #[command(about = "Get hash for workspace or projects")]
    Hash(HashCommand),

    #[command(about = "Generate machine readable CLI specification")]
    Declspec(DeclspecCommand),

    #[command(about = "Cache related subcommands")]
    Cache(CacheCommand),

    #[command(about = "Code generation related subcommands", alias = "gen")]
    Generator(GeneratorCommand),

    #[command(about = "Initialize a new workspace in the current directory")]
    Init(InitCommand),

    #[command(about = "Project related commands")]
    Project(ProjectCommand),

    #[command(about = "Start an MCP server for AI agent integration")]
    Mcp(McpCommand),
}
