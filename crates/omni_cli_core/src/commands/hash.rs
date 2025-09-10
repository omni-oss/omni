use clap::Subcommand;
use tracing_futures::WithSubscriber;

use crate::{context::Context, tracer::noop_subscriber};

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
    }

    Ok(())
}
