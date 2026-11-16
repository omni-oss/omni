use eyre::WrapErr as _;
use std::{collections::HashMap, time::Duration};

use base64::Engine;
use bytesize::ByteSize;
use clap::Subcommand;
use itertools::Itertools;
use omni_api::{
    CachePruneRequest, CacheRemoteSetupRequest, CacheStatsRequest, OmniApi,
};
use omni_cache::PrunedCacheEntry;
use omni_context::Context;
use omni_messages::NoopSubscriber;
use owo_colors::OwoColorize;

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
    #[command(
        about = "Generate an Ed25519 key pair for signing shared remote cache entries"
    )]
    Keygen,
    #[command(about = "Remote cache related commands")]
    Remote {
        // Boxed: the setup flags make this variant much larger than the others.
        #[command(flatten)]
        args: Box<RemoteArgs>,
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

    #[arg(
        long,
        help = "Filter the cache entries by the directory the owning project resides in, accepts glob patterns. Only matches tasks present in the current workspace"
    )]
    dir: Vec<String>,

    #[arg(
        long,
        short,
        help = "Filter the cache entries by the task meta configuration, accepts CEL syntax. Only matches tasks present in the current workspace"
    )]
    meta: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct PruneArgs {
    #[arg(long, short, default_value = "false", action = clap::ArgAction::SetTrue, help = "Add filter to clear only stale cache entries")]
    stale_only: bool,

    #[arg(long, short, help = "Add filter to clear only stale cache entries", value_parser = humantime::parse_duration)]
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
        help = "Add filter to clear only cache entries whose task meta configuration matches the given expression, accepts CEL syntax. Only matches tasks present in the current workspace"
    )]
    meta: Option<String>,

    #[arg(
        long,
        help = "Add filter to clear only cache entries of projects residing in the given directory, accepts glob patterns"
    )]
    dir: Vec<String>,

    #[arg(long, short, action = clap::ArgAction::SetTrue, help = "Prune the cache without prompting for confirmation")]
    yes: bool,

    #[arg(long, short, action = clap::ArgAction::SetTrue, default_value_t = false, help = "Show the cache entries that would be deleted", conflicts_with = "yes")]
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
#[command(group = clap::ArgGroup::new("signing_key_source").multiple(false))]
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

    #[arg(
        long,
        help = "Upload successful results to the remote cache (off by default)",
        default_value_t = false
    )]
    pub push: bool,

    #[arg(
        long,
        value_parser = ["isolated", "shared"],
        default_value = "isolated",
        help = "Where uploads go. `shared` needs the publish:artifacts scope"
    )]
    pub target: String,

    #[arg(
        long,
        default_value = "auto",
        value_parser = parse_scope,
        help = "The isolated scope label: `auto` (from the CI environment), `run` (the CI run id) or a literal such as `release`"
    )]
    pub scope: omni_configurations::ScopeConfig,

    #[arg(
        long,
        value_parser = clap::value_parser!(u64).range(1..),
        help = "Largest artifact to upload or download, in bytes. The server's own limit can only lower it"
    )]
    pub max_artifact_size: Option<u64>,

    #[arg(
        long,
        requires = "signing_key_source",
        help = "The id of this machine's signing key (see `omni cache keygen`); publishers only"
    )]
    pub signing_key_id: Option<String>,

    #[arg(
        long,
        group = "signing_key_source",
        requires = "signing_key_id",
        help = "The environment variable that holds the base64 private signing key"
    )]
    pub signing_key_env: Option<String>,

    #[arg(
        long,
        group = "signing_key_source",
        requires = "signing_key_id",
        help = "A file that holds the base64 private signing key"
    )]
    pub signing_key_file: Option<std::path::PathBuf>,

    #[arg(
        long,
        value_parser = ["auto", "off", "warn", "require"],
        help = "Verify signatures of shared entries; can only tighten the workspace's setting"
    )]
    pub verify: Option<String>,

    #[arg(
        long = "trusted-key",
        help = "Trust only this key id (repeatable); it must be one the workspace trusts"
    )]
    pub trusted_key: Vec<String>,
}

fn parse_scope(text: &str) -> Result<omni_configurations::ScopeConfig, String> {
    omni_configurations::ScopeConfig::parse(text)
}

pub async fn run(command: &CacheCommand, ctx: &Context) -> eyre::Result<()> {
    let api = OmniApi::new_with_sys(ctx.clone(), NoopSubscriber);

    match &command.subcommand {
        CacheSubcommands::Dir => {
            println!("{}", api.cache_dir().await.display());
        }

        CacheSubcommands::Keygen => {
            let pair = omni_remote_cache_client::generate_keypair()?;
            // The private key is shown once and never stored by omni: keep it in
            // the publisher's secret store, never in the repository.
            println!("Key id: {}", pair.id);
            println!();
            println!("Public key (commit this in workspace.omni.yaml):");
            println!("remote_cache:");
            println!("  signing:");
            println!("    trusted_keys:");
            println!("      - id: {}", pair.id);
            println!("        public_key: \"{}\"", pair.public_key);
            println!();
            println!(
                "Private key (secret; shown once; for the publishing job only):"
            );
            println!("{}", pair.private_key.as_str());
        }

        CacheSubcommands::Stats { args } => {
            let stats = api
                .cache_stats(CacheStatsRequest {
                    project: args.project.clone(),
                    task: args.task.clone(),
                    dir: args.dir.clone(),
                    meta: args.meta.clone(),
                })
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
                                        .format(&time::format_description::well_known::Rfc3339)
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
        }

        CacheSubcommands::Prune { args } => {
            prune(&api, args).await?;
        }

        CacheSubcommands::Remote { args } => match args.subcommand {
            RemoteSubcommands::Setup { ref args } => {
                api.cache_remote_setup(CacheRemoteSetupRequest {
                    api_base_url: args.api_base_url.clone(),
                    api_key: args.api_key.clone(),
                    tenant: args.tenant.clone(),
                    org: args.org.clone(),
                    ws: args.ws.clone(),
                    env: args.env.clone(),
                    secure: args.secure,
                    push: args.push,
                    target: if args.target == "shared" {
                        omni_configurations::RemoteTarget::Shared
                    } else {
                        omni_configurations::RemoteTarget::Isolated
                    },
                    scope: args.scope.clone(),
                    max_artifact_size: args.max_artifact_size,
                    signing_key: args.signing_key_id.clone().map(|id| {
                        omni_configurations::SigningKeyConfiguration {
                            id,
                            private_key_env: args.signing_key_env.clone(),
                            private_key_file: args.signing_key_file.clone(),
                        }
                    }),
                    verify: args.verify.as_deref().map(|v| match v {
                        "off" => omni_configurations::VerifyMode::Off,
                        "warn" => omni_configurations::VerifyMode::Warn,
                        "require" => omni_configurations::VerifyMode::Require,
                        _ => omni_configurations::VerifyMode::Auto,
                    }),
                    trusted_keys: (!args.trusted_key.is_empty())
                        .then(|| args.trusted_key.clone()),
                })
                .await
                .wrap_err(
                    "Failed to setup remote caching. Please check your credentials and try again.",
                )?;
            }
        },
    }

    Ok(())
}

async fn prune(
    api: &OmniApi<system_traits::impls::RealSys, omni_messages::NoopSubscriber>,
    args: &PruneArgs,
) -> eyre::Result<()> {
    tracing::debug!(?args, "prune");

    // Step 1: get the candidate list (always dry-run first).
    let prune_result = api
        .cache_prune(CachePruneRequest {
            stale_only: args.stale_only,
            older_than: args.older_than,
            larger_than: args.larger_than,
            project: args.project.clone(),
            task: args.task.clone(),
            meta: args.meta.clone(),
            dir: args.dir.clone(),
            dry_run: true,
        })
        .await?;

    let pruned = prune_result.entries;

    if pruned.is_empty() {
        println!("No cache entries matched the given filters");
        return Ok(());
    }

    // Display the entries.
    if !args.dry_run {
        println!("--- Cache Entries ---");
    }
    display_pruned_entries(&pruned);

    let pruned_count = pruned.len();
    let project_count = pruned
        .iter()
        .map(|e| e.project_name.as_str())
        .collect::<std::collections::HashSet<_>>()
        .len();

    if args.dry_run {
        println!(
            "Dry mode enabled, would prune {} cache entries from {} projects",
            pruned_count, project_count,
        );
        return Ok(());
    }

    // Step 2: confirm and actually prune.
    if !args.yes {
        println!(
            "Are you sure you want to prune the cache ({} entries from {} projects)? [y/N]",
            pruned_count, project_count
        );
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if input.trim() != "y" {
            println!("Aborting");
            return Ok(());
        }
        println!("Proceeding to prune the cache");
    }

    api.cache_force_prune(pruned).await?;
    println!(
        "{}",
        format!(
            "Pruned {} cache entries from {} projects",
            pruned_count, project_count
        )
        .red()
    );

    Ok(())
}

fn display_pruned_entries(pruned: &[PrunedCacheEntry]) {
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

    for (project_name, entries) in grouped {
        println!("Project: {}", project_name);
        for (task_name, entries) in entries {
            let task_name =
                task_name.split('#').nth(1).expect("should be some");
            println!("  Task: {}", task_name);
            for entry in entries {
                let hash = base64::engine::general_purpose::STANDARD
                    .encode(entry.digest.as_ref());
                println!(
                    "   {} {}({})",
                    hash,
                    match entry.stale {
                        omni_cache::StaleStatus::Unknown => "",
                        omni_cache::StaleStatus::Stale => "(stale) ",
                        omni_cache::StaleStatus::Fresh => "(fresh) ",
                    },
                    entry.size
                );
            }
        }
        println!();
    }
}

#[cfg(test)]
mod signing_flag_tests {
    use clap::Parser;

    use super::*;

    #[derive(Parser)]
    struct Cli {
        #[command(flatten)]
        args: SetupArgs,
    }

    fn parse(extra: &[&str]) -> Result<Cli, clap::Error> {
        let mut argv = vec![
            "t", "-b", "http://h", "-a", "k", "-t", "t", "-o", "o", "-w", "w",
        ];
        argv.extend_from_slice(extra);
        Cli::try_parse_from(argv)
    }

    #[test]
    fn a_signing_key_needs_an_id_and_exactly_one_source() {
        let id = "0123456789abcdef";
        assert!(parse(&[]).is_ok());
        assert!(
            parse(&["--signing-key-id", id, "--signing-key-env", "K"]).is_ok()
        );
        assert!(
            parse(&["--signing-key-id", id, "--signing-key-file", "k"]).is_ok()
        );
        assert!(parse(&["--signing-key-id", id]).is_err());
        assert!(parse(&["--signing-key-env", "K"]).is_err());
        assert!(parse(&["--signing-key-file", "k"]).is_err());
        assert!(
            parse(&[
                "--signing-key-id",
                id,
                "--signing-key-env",
                "K",
                "--signing-key-file",
                "k",
            ])
            .is_err()
        );
        assert!(parse(&["--verify", "strict"]).is_err());
        assert!(
            parse(&[
                "--verify",
                "require",
                "--trusted-key",
                id,
                "--trusted-key",
                "ab"
            ])
            .is_ok()
        );
    }
}
