use clap::Subcommand;

use crate::context::Context;

#[derive(clap::Args)]
#[command(author, version, about = "Cache management commands")]
pub struct CacheCommand {
    #[command(subcommand)]
    subcommand: CacheSubcommands,
}

#[derive(Subcommand)]
pub enum CacheSubcommands {
    #[command(about = "Print the cache directory")]
    Dir,
    #[command(about = "Show statistics about the cache")]
    Stats,
    #[command(about = "Prune the cache")]
    Prune {
        #[command(flatten)]
        args: PruneArgs,
    },
}

#[derive(clap::Args)]
pub struct PruneArgs {
    #[arg(
        long,
        short,
        default_value = "false",
        help = "Clear all cache entries, conflicts with filter flags",
        action = clap::ArgAction::SetTrue,
        conflicts_with_all = ["stale_only", "meta", "project", "task"],
    )]
    all: bool,

    #[arg(
        long,
        short,
        default_value = "false",
        action = clap::ArgAction::SetTrue,
        help = "Add filter to clear only stale cache entries",
    )]
    stale_only: bool,

    #[arg(
        short,
        long,
        help = "Add filter to clear only cache entries belonging to a project or task that matches the given meta"
    )]
    meta: Option<String>,

    #[arg(
        long,
        short,
        help = "Add filter to clear only cache entries belonging to a project that matches the given project name, accepts glob patterns"
    )]
    project: Option<String>,

    #[arg(
        long,
        short,
        help = "Add filter to clear only cache entries belonging to a task that matches the given task name, accepts glob patterns"
    )]
    task: Option<String>,

    #[arg(
        long,
        short,
        action = clap::ArgAction::SetTrue,
        default_value_t = false,
        help = "Show the cache entries that would be deleted",
    )]
    dry_run: bool,
}

pub async fn run(command: &CacheCommand, ctx: &Context) -> eyre::Result<()> {
    match &command.subcommand {
        CacheSubcommands::Dir => {
            println!("{}", ctx.cache_dir().display());
        }
        CacheSubcommands::Stats => {
            stats(ctx).await?;
        }
        CacheSubcommands::Prune { args } => {
            prune(ctx, args).await?;
        }
    }

    Ok(())
}

async fn stats(_ctx: &Context) -> eyre::Result<()> {
    Ok(())
}

async fn prune(_ctx: &Context, _args: &PruneArgs) -> eyre::Result<()> {
    Ok(())
}
