use clap::{Args, Subcommand};
use omni_api::OmniApi;
use omni_context::Context;
use omni_messages::NoopSubscriber;
use omni_tracing_subscriber::noop_subscriber;
use tracing_futures::WithSubscriber as _;

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
    raw: bool,
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
    let api = OmniApi::new_with_sys(ctx.clone(), NoopSubscriber);

    let response = if command.args.raw {
        match command.subcommand {
            HashSubcommands::Workspace => {
                api.hash_workspace()
                    .with_subscriber(noop_subscriber())
                    .await?
            }
            HashSubcommands::Project { ref command } => {
                api.hash_project(&command.project, &command.task)
                    .with_subscriber(noop_subscriber())
                    .await?
            }
        }
    } else {
        match command.subcommand {
            HashSubcommands::Workspace => api.hash_workspace().await?,
            HashSubcommands::Project { ref command } => {
                api.hash_project(&command.project, &command.task).await?
            }
        }
    };

    if command.args.raw {
        print!("{}", response.hash);
    } else {
        println!("{}", response.hash);
    }

    Ok(())
}
