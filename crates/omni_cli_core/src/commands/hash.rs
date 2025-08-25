use clap::Subcommand;
use tracing_futures::WithSubscriber;

use crate::{context::Context, tracer::noop_subscriber};

#[derive(clap::Args)]
pub struct HashCommand {
    #[command(subcommand)]
    subcommand: HashSubcommands,
}

#[derive(Subcommand)]
pub enum HashSubcommands {
    Workspace,
}

pub async fn run(command: &HashCommand, ctx: &Context) -> eyre::Result<()> {
    let mut ctx = ctx.clone();

    ctx.load_projects()
        .with_subscriber(noop_subscriber())
        .await?;

    match command.subcommand {
        HashSubcommands::Workspace => {
            let hashstring = ctx.get_workspace_hash_string().await?;
            println!("{hashstring}");
        }
    }

    Ok(())
}
