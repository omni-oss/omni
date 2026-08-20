use std::path::PathBuf;

use omni_api::{OmniApi, ToolWorkingDir};
use omni_context::Context;
use omni_messages::NoopSubscriber;
use owo_colors::OwoColorize;

use super::parser::parse_key_value;

#[derive(Debug, Clone, clap::Args)]
pub struct ToolCommand {
    #[command(subcommand)]
    pub subcommand: ToolSubcommand,
}

#[derive(Debug, Clone, clap::Subcommand)]
pub enum ToolSubcommand {
    #[command(alias = "ls", about = "List available tools")]
    List(#[command(flatten)] ToolListCommand),

    #[command(about = "Show a tool's input schema")]
    Inspect(#[command(flatten)] ToolInspectCommand),

    #[command(about = "Run a tool")]
    Run(#[command(flatten)] ToolRunCommand),
}

#[derive(Debug, Clone, clap::Args)]
pub struct ToolListCommand {}

#[derive(Debug, Clone, clap::Args)]
pub struct ToolInspectCommand {
    #[arg(help = "Name of the tool to inspect")]
    pub name: String,

    #[arg(
        long,
        help = "Pretty-print the JSON output",
        default_value_t = false,
        action = clap::ArgAction::SetTrue
    )]
    pub pretty: bool,
}

#[derive(Debug, Clone, clap::Args)]
pub struct ToolRunCommand {
    #[arg(help = "Name of the tool to run")]
    pub name: String,

    #[arg(
        long = "args-json",
        help = "Tool arguments as a JSON object; merged under any --arg flags"
    )]
    pub args_json: Option<String>,

    #[arg(
        long = "arg",
        help = "A single k=v argument (repeatable); overrides matching --args-json keys. The value is parsed as JSON when possible, else treated as a string",
        value_parser = parse_key_value::<String, String>
    )]
    pub arg: Vec<(String, String)>,

    #[arg(
        long,
        help = "Directory the tool operates in (relative paths resolve here); mutually exclusive with --project",
        conflicts_with = "project"
    )]
    pub cwd: Option<PathBuf>,

    #[arg(
        long,
        short = 'p',
        help = "Run the tool in this project's directory; mutually exclusive with --cwd",
        conflicts_with = "cwd"
    )]
    pub project: Option<String>,

    #[arg(
        long,
        help = "Pretty-print the JSON output",
        default_value_t = false,
        action = clap::ArgAction::SetTrue
    )]
    pub pretty: bool,
}

pub async fn run(cmd: &ToolCommand, ctx: &Context) -> eyre::Result<()> {
    match &cmd.subcommand {
        ToolSubcommand::List(command) => run_tool_list(command, ctx).await,
        ToolSubcommand::Inspect(command) => {
            run_tool_inspect(command, ctx).await
        }
        ToolSubcommand::Run(command) => run_tool_run(command, ctx).await,
    }
}

async fn run_tool_list(
    _command: &ToolListCommand,
    ctx: &Context,
) -> eyre::Result<()> {
    let response = OmniApi::new_with_sys(ctx.clone(), NoopSubscriber)
        .tool_list()
        .await?;

    println!("{}", "Available Tools:".bold());
    for tool in response.tools {
        println!(
            "- {}{}{}",
            tool.name.bold(),
            if tool.description.is_some() { ": " } else { "" },
            tool.description.as_deref().unwrap_or(""),
        );
    }

    Ok(())
}

async fn run_tool_inspect(
    command: &ToolInspectCommand,
    ctx: &Context,
) -> eyre::Result<()> {
    let response = OmniApi::new_with_sys(ctx.clone(), NoopSubscriber)
        .tool_inspect(&command.name)
        .await?;

    if command.pretty {
        println!("{}", serde_json::to_string_pretty(&response.input_schema)?);
    } else {
        println!("{}", serde_json::to_string(&response.input_schema)?);
    }

    Ok(())
}

async fn run_tool_run(
    command: &ToolRunCommand,
    ctx: &Context,
) -> eyre::Result<()> {
    let args = merge_tool_args(command.args_json.as_deref(), &command.arg)?;

    // `--cwd` and `--project` are mutually exclusive (enforced by clap).
    let working_dir = match (&command.cwd, &command.project) {
        (Some(path), _) => Some(ToolWorkingDir::Path(path.clone())),
        (_, Some(project)) => Some(ToolWorkingDir::Project(project.clone())),
        (None, None) => None,
    };

    let response = OmniApi::new_with_sys(ctx.clone(), NoopSubscriber)
        .tool_run(&command.name, args, working_dir)
        .await?;

    if command.pretty {
        println!("{}", serde_json::to_string_pretty(&response)?);
    } else {
        println!("{}", serde_json::to_string(&response)?);
    }

    Ok(())
}

/// Merge the `--args-json` object with repeatable `--arg k=v` flags into a
/// single JSON object. `--arg` wins on key collisions. Each `--arg` value is
/// parsed as JSON when possible so scalars keep their type, otherwise it is
/// kept as a string. This dual-input merge is a CLI-only convenience.
fn merge_tool_args(
    args_json: Option<&str>,
    args: &[(String, String)],
) -> eyre::Result<serde_json::Value> {
    let mut merged = match args_json {
        Some(raw) => {
            let value: serde_json::Value = serde_json::from_str(raw)
                .map_err(|e| eyre::eyre!("invalid --args-json: {e}"))?;
            match value {
                serde_json::Value::Object(map) => map,
                _ => {
                    return Err(eyre::eyre!(
                        "--args-json must be a JSON object"
                    ));
                }
            }
        }
        None => serde_json::Map::new(),
    };

    for (key, raw) in args {
        let value = serde_json::from_str::<serde_json::Value>(raw)
            .unwrap_or_else(|_| serde_json::Value::String(raw.clone()));
        merged.insert(key.clone(), value);
    }

    Ok(serde_json::Value::Object(merged))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::merge_tool_args;

    #[test]
    fn arg_overrides_matching_json_key() {
        let merged = merge_tool_args(
            Some(r#"{"who":"world","keep":true}"#),
            &[("who".to_string(), "alice".to_string())],
        )
        .unwrap();
        assert_eq!(merged, json!({ "who": "alice", "keep": true }));
    }

    #[test]
    fn arg_value_is_parsed_as_json_when_possible() {
        let merged = merge_tool_args(
            None,
            &[
                ("times".to_string(), "5".to_string()),
                ("flag".to_string(), "true".to_string()),
                ("name".to_string(), "bob".to_string()),
            ],
        )
        .unwrap();
        assert_eq!(merged["times"], json!(5));
        assert_eq!(merged["flag"], json!(true));
        assert_eq!(merged["name"], json!("bob"));
    }

    #[test]
    fn empty_inputs_produce_an_empty_object() {
        let merged = merge_tool_args(None, &[]).unwrap();
        assert_eq!(merged, json!({}));
    }

    #[test]
    fn non_object_args_json_is_rejected() {
        let err = merge_tool_args(Some("[1,2,3]"), &[]);
        assert!(err.is_err());
    }
}
