use clap::{Args, Subcommand};
use omni_tracing_subscriber::noop_subscriber;
use tracing_futures::WithSubscriber;

use crate::context::Context;

#[derive(clap::Args)]
#[command(author, version, about = "Get hashes for the workspace or projects")]
pub struct HashCommand {
    #[command(subcommand)]
    subcommand: HashSubcommands,

    #[command(flatten)]
    args: HashArgs,
}

#[derive(clap::Args)]
pub struct HashArgs {
    #[arg(
        long,
        short = 'r',
        default_value_t = false,
        action = clap::ArgAction::SetTrue,
        help = "Only output the raw string representation of the hash, no trailing newline, and no traces"
    )]
    raw_value: bool,
}

#[derive(Subcommand)]
pub enum HashSubcommands {
    #[command(about = "Get the hash for the workspace")]
    Workspace,

    #[command(about = "Get the hash for a project")]
    Project {
        #[command(flatten)]
        command: HashProjectCommand,
    },
}

#[derive(Debug, Args)]
pub struct HashProjectCommand {
    #[arg(required = true, help = "Project name")]
    project: String,

    #[arg(long, short = 't', help = "Hash specific task")]
    task: Vec<String>,
}

pub async fn run(command: &HashCommand, ctx: &Context) -> eyre::Result<()> {
    let ctx = ctx.clone();

    let ctx = if command.args.raw_value {
        ctx.into_loaded().with_subscriber(noop_subscriber()).await?
    } else {
        ctx.into_loaded().await?
    };

    match command.subcommand {
        HashSubcommands::Workspace => {
            let hashstring = ctx.get_workspace_hash_string().await?;

            if command.args.raw_value {
                print!("{hashstring}");
            } else {
                println!("{hashstring}");
            }
        }
        HashSubcommands::Project {
            command: ref project_cmd,
        } => {
            let hashstring = ctx
                .get_project_hash_string(
                    &project_cmd.project,
                    project_cmd
                        .task
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .as_slice(),
                )
                .await?;

            if command.args.raw_value {
                print!("{hashstring}");
            } else {
                println!("{hashstring}");
            }
        }
    }

    Ok(())
}
