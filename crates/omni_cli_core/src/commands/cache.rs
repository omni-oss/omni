use std::{collections::HashMap, time::Duration};

use base64::Engine;
use bytesize::ByteSize;
use clap::Subcommand;
use itertools::Itertools;
use omni_cache::{PruneCacheArgs, PruneStaleOnly, TaskExecutionCacheStore};
use owo_colors::OwoColorize;

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

#[derive(clap::Args, Debug)]
pub struct PruneArgs {
    #[arg(
        long,
        short,
        default_value = "false",
        action = clap::ArgAction::SetTrue,
        help = "Add filter to clear only stale cache entries",
    )]
    stale_only: bool,

    #[arg(
        long,
        short,
        help = "Add filter to clear only stale cache entries",
        value_parser = humantime::parse_duration
    )]
    older_than: Option<Duration>,

    #[arg(
        long,
        short,
        help = "Add filter to clear only cache entries that are larger than the given size"
    )]
    larger_than: Option<ByteSize>,

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
        help = "Prune the cache without prompting for confirmation"
    )]
    yes: bool,

    #[arg(
        long,
        short,
        action = clap::ArgAction::SetTrue,
        default_value_t = false,
        help = "Show the cache entries that would be deleted",
        conflicts_with = "yes"
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

                println!("      File Sizes: {}", task.total_size);
                if let Some(log_file) = &task.log_file {
                    println!("        Log: {}", log_file.size);
                } else {
                    println!("        Log: N/A");
                }
                println!("        Meta: {}", task.meta_file.size);

                if task.cached_files.is_empty() {
                    println!("        Cached Files: N/A");
                } else {
                    println!(
                        "        Cached Files: {}",
                        task.cached_files_total_size
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

async fn prune(ctx: &Context, cli_args: &PruneArgs) -> eyre::Result<()> {
    trace::debug!(?cli_args, "prune");
    let cache_store = ctx.create_cache_store();

    let args = PruneCacheArgs::new(
        if cli_args.dry_run {
            true
        } else {
            !cli_args.yes
        },
        if cli_args.stale_only {
            // loaded_context.get_cache_info(project_name, task_name);
            PruneStaleOnly::On {}
        } else {
            PruneStaleOnly::Off
        },
        cli_args.older_than,
        cli_args.project.as_deref(),
        cli_args.task.as_deref(),
        cli_args.larger_than,
    );

    if cli_args.stale_only {
        trace::warn!("--stale-only flags currently non functional");
    }

    let pruned = cache_store.prune_caches(&args).await?;
    if pruned.is_empty() {
        trace::warn!("No cache entries matched the given filters");
    } else {
        if !cli_args.dry_run {
            trace::info!("--- Cache Entries ---");
        }

        let pruned_count = pruned.len();
        let grouped = pruned
            .iter()
            .into_group_map_by(|p| p.project_name.to_string())
            .into_iter()
            .map(|(project_name, entries)| {
                let entries = entries.into_iter().into_group_map_by(|e| {
                    format!("{}#{}", e.project_name, e.task_name)
                });

                (project_name, entries)
            })
            .collect::<HashMap<String, HashMap<String, _>>>();

        let project_count = grouped.len();
        for (project_name, entries) in grouped {
            println!("Project: {}", project_name);
            for (task_name, entries) in entries {
                let task_name =
                    task_name.split("#").nth(1).expect("should be some");
                println!("  Task: {}", task_name);
                for entry in entries {
                    let hash = base64::engine::general_purpose::STANDARD
                        .encode(&entry.execution_hash);
                    println!(
                        "   {} {}({})",
                        hash,
                        (match entry.stale {
                            omni_cache::StaleStatus::Unknown => {
                                ""
                            }
                            omni_cache::StaleStatus::Stale => {
                                "(stale) "
                            }
                            omni_cache::StaleStatus::Fresh => {
                                "(fresh) "
                            }
                        })
                        .to_string(),
                        entry.size
                    );
                }
            }

            println!();
        }

        let should_confirm = if cli_args.dry_run {
            false
        } else {
            !cli_args.yes
        };

        if should_confirm {
            println!("Are you sure you want to prune the cache? [y/N]");
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            if input.trim() != "y" {
                println!("Aborting");
                return Ok(());
            } else {
                println!("Proceeding to prune the cache");
            }
        }

        if !cli_args.dry_run {
            cache_store.force_prune_caches(&pruned).await?;
            trace::info!(
                "{}",
                format!(
                    "Pruned {} cache entries from {} projects",
                    pruned_count, project_count
                )
                .red()
            );
        } else {
            trace::info!(
                "Dry mode enabled, would prune {} cache entries from {} projects",
                pruned_count,
                project_count
            );
        }
    }
    Ok(())
}
