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
        cache::CacheCommand, code::CodeCommand, declspec::DeclspecCommand,
        diagnostics::DiagnosticsCommand, generator::GeneratorCommand,
        hash::HashCommand, ignore::IgnoreCommand, init::InitCommand,
        project::ProjectCommand, projection::ProjectionCommand,
        remote_source::RemoteSourcesCommand, tool::ToolCommand,
    },
};

pub mod cache;
pub mod code;
pub mod completion;
pub mod config;
pub mod declspec;
pub mod diagnostics;
pub mod env;
pub mod exec;

pub mod generator;
mod generator_utils;
pub mod hash;
pub mod ignore;
pub mod init;
pub mod mcp;
pub mod pack;
pub mod project;
pub mod projection;
pub mod remote_source;
pub mod run;
pub mod tool;

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

/// How a top-level failure is printed (RFC 0021).
#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ErrorFormat {
    #[default]
    Text,
    Json,
}

/// CLI presentation settings passed to commands, independent of workspace state.
#[derive(Args, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OutputOptions {
    /// Hide omni's narration, but keep results, failures and essential notices.
    #[arg(
        short = 'q',
        long,
        help = "Hide omni's own narration: lifecycle lines, progress, and warning/info notices. Results, failures, task output and essential notices still show",
        default_value_t = false,
        action = clap::ArgAction::SetTrue,
        env = "OMNI_QUIET",
        global = true
    )]
    pub quiet: bool,
}

#[cfg(test)]
mod output_options_tests {
    use clap::Parser;

    use super::{Cli, CliArgs, OutputOptions};

    #[test]
    fn narration_is_enabled_by_default() {
        assert_eq!(CliArgs::default().output, OutputOptions::default());
        assert!(!OutputOptions::default().quiet);
    }

    #[test]
    fn quiet_is_flattened_and_global() {
        for args in [
            vec!["omni", "-q", "env", "all"],
            vec!["omni", "env", "all", "--quiet"],
        ] {
            let cli = Cli::try_parse_from(args).unwrap();
            assert!(cli.args.output.quiet);
        }
    }
}

#[derive(Args, Debug)]
#[command()]
pub struct CliArgs {
    #[command(flatten)]
    pub output: OutputOptions,

    #[arg(
        long = "trace",
        value_name = "FILTER",
        help = "Write developer traces to stderr, using tracing filter directives (for example `omni_cache=debug` or `debug`)",
        env = "OMNI_TRACE"
    )]
    pub trace: Option<String>,

    #[arg(
        long = "trace-file",
        help = "The file to write traces to (default: ./.omni/trace/omni.log, except in shared MCP mode)"
    )]
    pub trace_file: Option<PathBuf>,

    #[arg(
        long = "trace-file-level",
        help = "The trace level to use for file traces",
        value_enum,
        default_value = "off",
        env = "OMNI_TRACE_FILE_LEVEL"
    )]
    pub trace_file_level: EnumValueAdapter<Level>,

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

    #[arg(
        long = "error-format",
        help = "How to print a failure: `text` for people, `json` for scripts and CI",
        env = "OMNI_ERROR_FORMAT",
        value_enum,
        default_value = "text",
        global = true
    )]
    pub error_format: ErrorFormat,

    #[arg(
        long = "cache-dir",
        help = "The directory to use for the cache",
        env = "OMNI_CACHE_DIR",
        global = true
    )]
    pub cache_dir: Option<PathBuf>,
}

impl Default for CliArgs {
    fn default() -> Self {
        Self {
            output: OutputOptions::default(),
            trace: None,
            trace_file: None,
            trace_file_level: EnumValueAdapter::new(Level::Off),
            env_root_dir_marker: None,
            env_file: None,
            env: None,
            inherit_env_vars: false,
            cache_dir: None,
            error_format: ErrorFormat::Text,
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

    #[command(about = "Inspect diagnostic codes")]
    Diagnostics(DiagnosticsCommand),

    #[command(about = "Look up error and notice codes")]
    Code(CodeCommand),

    #[command(about = "Cache related subcommands")]
    Cache(CacheCommand),

    #[command(about = "Code generation related subcommands", alias = "gen")]
    Generator(GeneratorCommand),

    #[command(about = "Initialize a new workspace in the current directory")]
    Init(InitCommand),

    #[command(about = "Project related commands")]
    Project(ProjectCommand),

    #[command(
        about = "Materialize files from sources into the workspace via links"
    )]
    Projection(ProjectionCommand),

    #[command(about = "Inspect source packs declared in the workspace")]
    Pack(pack::PackCommand),

    #[command(about = "Tool related subcommands")]
    Tool(ToolCommand),

    #[command(
        about = "Prefetch and pin remote sources for every subsystem",
        alias = "remote-source"
    )]
    RemoteSources(RemoteSourcesCommand),

    #[command(
        about = "Maintain omni's managed block in ignore files (.gitignore, .ignore, .omniignore)"
    )]
    Ignore(IgnoreCommand),

    #[command(about = "Start an MCP server for AI agent integration")]
    Mcp(McpCommand),
}
