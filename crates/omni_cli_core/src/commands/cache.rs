use bytesize::ByteSize;
use clap::Subcommand;
use omni_cache::TaskExecutionCacheStore;

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
    Stats {
        #[command(flatten)]
        args: StatsArgs,
    },
    #[command(about = "Prune the cache")]
    Prune {
        #[command(flatten)]
        args: PruneArgs,
    },
}

#[derive(clap::Args)]
pub struct StatsArgs {
    #[arg(
        long,
        short,
        help = "Filter the cache entries by project name, accepts glob patterns"
    )]
    project: Option<String>,

    #[arg(
        long,
        short,
        help = "Filter the cache entries by task name, accepts glob patterns"
    )]
    task: Option<String>,
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
        CacheSubcommands::Stats { args } => {
            stats(ctx, args).await?;
        }
        CacheSubcommands::Prune { args } => {
            prune(ctx, args).await?;
        }
    }

    Ok(())
}

async fn stats(ctx: &Context, args: &StatsArgs) -> eyre::Result<()> {
    let cache_store = ctx.create_cache_store();
    let stats = cache_store
        .get_stats(args.project.as_deref(), args.task.as_deref())
        .await?;

    for (i, project) in stats.projects.iter().enumerate() {
        println!("Project: {}", project.project_name);
        println!("  Tasks:");
        if project.tasks.is_empty() {
            println!("    (No tasks)");
        } else {
            for task in &project.tasks {
                println!("    - Task: {}", task.task_name);
                println!(
                    "      Created: {}",
                    task.created_timestamp
                        .format(&time::format_description::well_known::Rfc3339)
                        .unwrap()
                );

                if let Some(last_used) = task.last_used_timestamp {
                    println!(
                        "      Last Used: {}",
                        last_used
                            .format(
                                &time::format_description::well_known::Rfc3339
                            )
                            .unwrap()
                    );
                } else {
                    println!("      Last Used: N/A");
                }
                let cached_files_total = task
                    .cached_files
                    .iter()
                    .map(|f| f.size.as_u64())
                    .sum::<u64>();
                let meta_total = task.meta_file.size.as_u64();
                let log_total =
                    task.log_file.as_ref().map_or(0, |f| f.size.as_u64());
                let total =
                    ByteSize::b(cached_files_total + meta_total + log_total);

                println!("      File Sizes: {total}");
                if let Some(log_file) = &task.log_file {
                    println!("        Log: {}", log_file.size);
                } else {
                    println!("        Log: N/A");
                }
                println!("        Meta: {}", task.meta_file.size);

                if task.cached_files.is_empty() {
                    println!("        Cached Files: N/A",);
                } else {
                    println!(
                        "        Cached Files: {}",
                        ByteSize::b(cached_files_total)
                    );
                }
            }
        }

        if i != stats.projects.len() - 1 {
            println!();
        }
    }

    Ok(())
}

async fn prune(_ctx: &Context, _args: &PruneArgs) -> eyre::Result<()> {
    Ok(())
}
