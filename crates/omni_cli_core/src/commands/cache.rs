use std::{collections::HashMap, time::Duration};

use base64::Engine;
use bytesize::ByteSize;
use clap::Subcommand;
use derive_new::new;
use itertools::Itertools;
use omni_cache::Context as ContextTrait;
use omni_cache::{PruneCacheArgs, PruneStaleOnly, TaskExecutionCacheStore};
use omni_context::{ContextSys, EnvVarsMap, LoadedContext, LoadedContextError};
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
    #[command(about = "Prune the local cache")]
    Prune {
        #[command(flatten)]
        args: PruneArgs,
    },
    #[command(about = "Remote cache related commands")]
    Remote {
        #[command(flatten)]
        args: RemoteArgs,
    },
}

#[derive(clap::Args)]
pub struct StatsArgs {
    #[arg(
        long,
        short,
        help = "Filter the cache entries by project name, accepts glob patterns"
    )]
    project: Vec<String>,

    #[arg(
        long,
        short,
        help = "Filter the cache entries by task name, accepts glob patterns"
    )]
    task: Vec<String>,
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
    project: Vec<String>,

    #[arg(
        long,
        short,
        help = "Add filter to clear only cache entries belonging to a task that matches the given task name, accepts glob patterns"
    )]
    task: Vec<String>,

    #[arg(
        long,
        short,
        help = "Add filter to clear only cache entries of projects residing in the given directory, accepts glob patterns"
    )]
    dir: Vec<String>,

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

#[derive(clap::Args, Debug)]
pub struct RemoteArgs {
    #[clap(subcommand)]
    subcommand: RemoteSubcommands,
}

#[derive(Subcommand, Debug)]
pub enum RemoteSubcommands {
    #[command(about = "Setup remote caching")]
    Setup {
        #[command(flatten)]
        args: SetupArgs,
    },
}

#[derive(clap::Args, Debug)]
pub struct SetupArgs {
    #[arg(
        long,
        short = 'b',
        help = "The endpoint base URL of the remote cache server"
    )]
    pub api_base_url: String,

    #[arg(long, short, help = "The API key of the remote cache server")]
    pub api_key: String,

    #[arg(long, short, help = "The tenant code of the remote cache server")]
    pub tenant: String,

    #[arg(
        long,
        short,
        help = "The organization code of the remote cache server"
    )]
    pub org: String,

    #[arg(long, short, help = "The workspace code of the remote cache server")]
    pub ws: String,

    #[arg(
        long,
        short,
        help = "The environment code of the remote cache server"
    )]
    pub env: Option<String>,

    #[arg(
        long,
        short,
        help = "Encrypt the remote cache configuration file",
        default_value_t = false
    )]
    pub secure: bool,
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
        CacheSubcommands::Remote { args } => {
            remote(ctx, args).await?;
        }
    }

    Ok(())
}

async fn stats(ctx: &Context, args: &StatsArgs) -> eyre::Result<()> {
    let cache_store = ctx.create_cache_store();
    let stats = cache_store
        .get_stats(
            args.project
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .as_slice(),
            args.task
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .as_slice(),
        )
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

    let loaded_context;

    let projects = cli_args
        .project
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>();
    let tasks = cli_args.task.iter().map(|s| s.as_str()).collect::<Vec<_>>();
    let dirs = cli_args.dir.iter().map(|s| s.as_str()).collect::<Vec<_>>();

    let args = PruneCacheArgs::new(
        if cli_args.dry_run {
            true
        } else {
            !cli_args.yes
        },
        if cli_args.stale_only {
            loaded_context = ctx.clone().into_loaded().await?;
            // loaded_context.get_cache_info(project_name, task_name);
            PruneStaleOnly::new_on(ContextWrapper::new(&loaded_context))
        } else {
            PruneStaleOnly::new_off()
        },
        cli_args.older_than,
        projects.as_slice(),
        dirs.as_slice(),
        tasks.as_slice(),
        cli_args.larger_than,
    );

    if cli_args.stale_only {
        trace::warn!(
            "--stale-only flag is functional but currently experimental"
        );
    }

    let pruned = cache_store.prune_caches(&args).await?;
    if pruned.is_empty() {
        trace::warn!("No cache entries matched the given filters");
    } else {
        if !cli_args.dry_run {
            println!("--- Cache Entries ---");
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
                        .encode(&entry.digest);
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
            println!(
                "Are you sure you want to prune the cache ({} entries from {} projects)? [y/N]",
                pruned_count, project_count
            );
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

async fn remote(ctx: &Context, cli_args: &RemoteArgs) -> eyre::Result<()> {
    match cli_args.subcommand {
        RemoteSubcommands::Setup { ref args } => {
            remote_setup(ctx, args).await?;
        }
    }

    Ok(())
}

async fn remote_setup(ctx: &Context, cli_args: &SetupArgs) -> eyre::Result<()> {
    let client = ctx.create_remote_cache_client();
    let ext = if cli_args.secure { "enc" } else { "yaml" };
    let config_path = ctx.remote_cache_configuration_path(ext);

    omni_setup::setup_remote_caching_config(
        &client,
        config_path.as_path(),
        &cli_args.api_base_url,
        &cli_args.api_key,
        &cli_args.tenant,
        &cli_args.org,
        &cli_args.ws,
        cli_args.env.as_deref(),
        cli_args.secure,
    )
    .await.inspect_err(|_| {
        trace::error!("Failed to setup remote caching. Please check your credentials and try again.");
    })?;
    Ok(())
}

#[derive(new)]
#[repr(transparent)]
struct ContextWrapper<'a, TSys: ContextSys> {
    context: &'a LoadedContext<TSys>,
}

impl<'a, TSys: ContextSys> ContextTrait for ContextWrapper<'a, TSys> {
    type Error = LoadedContextError;

    fn get_project_meta_config(
        &self,
        project_name: &str,
    ) -> Option<&omni_configurations::MetaConfiguration> {
        self.context.get_project_meta_config(project_name)
    }

    fn get_task_meta_config(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> Option<&omni_configurations::MetaConfiguration> {
        self.context.get_task_meta_config(project_name, task_name)
    }

    fn get_project_graph(
        &self,
    ) -> Result<omni_core::ProjectGraph, Self::Error> {
        self.context.get_project_graph()
    }

    fn projects(&self) -> &[omni_core::Project] {
        self.context.projects()
    }

    fn get_task_env_vars(
        &self,
        node: &omni_core::TaskExecutionNode,
    ) -> Result<Option<std::sync::Arc<EnvVarsMap>>, Self::Error> {
        self.context.get_task_env_vars(node)
    }

    fn get_cache_info(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> Option<&omni_task_context::CacheInfo> {
        self.context.get_cache_info(project_name, task_name)
    }

    fn root_dir(&self) -> &std::path::Path {
        self.context.root_dir()
    }
}
