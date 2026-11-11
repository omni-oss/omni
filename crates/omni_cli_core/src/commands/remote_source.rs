use std::process::ExitCode;

use std::time::Duration;

use bytesize::ByteSize;
use omni_api::{
    OmniApi, RemoteSourcesInstallRequest, RemoteSourcesPruneRequest,
    SubsystemSelection,
};
use omni_context::Context;
use omni_messages::NoopSubscriber;

#[derive(Debug, Clone, clap::Args)]
pub struct RemoteSourcesCommand {
    #[command(subcommand)]
    pub subcommand: RemoteSourcesSubcommand,
}

#[derive(Debug, Clone, clap::Subcommand)]
pub enum RemoteSourcesSubcommand {
    #[command(
        about = "Prefetch and pin every remote source declared in the workspace"
    )]
    Install(#[command(flatten)] RemoteSourcesInstallArgs),

    #[command(
        about = "Delete checkouts from the remote-source store by age or size. They are refetched when next used"
    )]
    Prune(#[command(flatten)] RemoteSourcesPruneArgs),
}

#[derive(Debug, Clone, clap::Args)]
pub struct RemoteSourcesPruneArgs {
    #[arg(
        long,
        short,
        help = "Only checkouts last used longer ago than this (for example 30d)",
        value_parser = humantime::parse_duration
    )]
    pub older_than: Option<Duration>,

    #[arg(
        long,
        short,
        help = "Only checkouts larger than this (for example 100MB)"
    )]
    pub larger_than: Option<ByteSize>,

    #[arg(
        long,
        action = clap::ArgAction::SetTrue,
        help = "Select every checkout in the store"
    )]
    pub all: bool,

    #[arg(
        long,
        short,
        action = clap::ArgAction::SetTrue,
        help = "Prune without prompting for confirmation"
    )]
    pub yes: bool,

    #[arg(
        long,
        short,
        action = clap::ArgAction::SetTrue,
        help = "Show the checkouts that would be deleted",
        conflicts_with = "yes"
    )]
    pub dry_run: bool,
}

#[derive(Debug, Clone, clap::Args)]
pub struct RemoteSourcesInstallArgs {
    #[arg(
        long,
        help = "Advance mutable refs by re-resolving branches or tags and re-pinning them",
        default_value_t = false,
        action = clap::ArgAction::SetTrue
    )]
    pub update: bool,

    #[arg(
        long,
        help = "Process generator sources",
        default_value_t = false,
        action = clap::ArgAction::SetTrue
    )]
    pub generators: bool,

    #[arg(
        long,
        help = "Process tool sources",
        default_value_t = false,
        action = clap::ArgAction::SetTrue
    )]
    pub tools: bool,

    #[arg(
        long,
        help = "Process projection sources",
        default_value_t = false,
        action = clap::ArgAction::SetTrue
    )]
    pub projections: bool,
}

pub async fn run(
    cmd: &RemoteSourcesCommand,
    ctx: &Context,
) -> eyre::Result<ExitCode> {
    match &cmd.subcommand {
        RemoteSourcesSubcommand::Install(args) => run_install(args, ctx).await,
        RemoteSourcesSubcommand::Prune(args) => run_prune(args, ctx).await,
    }
}

async fn run_prune(
    args: &RemoteSourcesPruneArgs,
    ctx: &Context,
) -> eyre::Result<ExitCode> {
    let api = OmniApi::new_with_sys(ctx.clone(), NoopSubscriber);
    let request = |dry_run| RemoteSourcesPruneRequest {
        all: args.all,
        older_than: args.older_than,
        larger_than: args.larger_than,
        dry_run,
    };

    let candidates = api.remote_sources_prune(request(true)).await?;
    if candidates.checkouts.is_empty() {
        println!("No store checkouts matched the given filters");
        return Ok(ExitCode::SUCCESS);
    }
    let total: u64 = candidates.checkouts.iter().map(|c| c.size_bytes).sum();
    for checkout in &candidates.checkouts {
        println!(
            "  {} {} ({}, unused for {})",
            checkout.remote,
            checkout.commit,
            ByteSize(checkout.size_bytes),
            humantime::format_duration(Duration::from_secs(
                checkout.unused_for_secs
            )),
        );
    }

    if args.dry_run {
        println!(
            "Dry mode enabled, would prune {} checkouts ({})",
            candidates.checkouts.len(),
            ByteSize(total)
        );
        return Ok(ExitCode::SUCCESS);
    }

    if !args.yes {
        println!(
            "Are you sure you want to prune {} checkouts ({})? [y/N]",
            candidates.checkouts.len(),
            ByteSize(total)
        );
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if input.trim() != "y" {
            println!("Aborting");
            return Ok(ExitCode::SUCCESS);
        }
    }

    let pruned = api.remote_sources_prune(request(false)).await?;
    println!("Pruned {} checkouts", pruned.checkouts.len());
    Ok(ExitCode::SUCCESS)
}

async fn run_install(
    args: &RemoteSourcesInstallArgs,
    ctx: &Context,
) -> eyre::Result<ExitCode> {
    let response = OmniApi::new_with_sys(ctx.clone(), NoopSubscriber)
        .remote_sources_install(RemoteSourcesInstallRequest {
            update: args.update,
            select: SubsystemSelection {
                generators: args.generators,
                tools: args.tools,
                projections: args.projections,
            },
        })
        .await?;

    for subsystem in &response.subsystems {
        println!(
            "  {}: {} materialized, {} deduplicated",
            subsystem.subsystem, subsystem.materialized, subsystem.deduplicated
        );
    }

    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: RemoteSourcesSubcommand,
    }

    fn parse(args: &[&str]) -> RemoteSourcesInstallArgs {
        let RemoteSourcesSubcommand::Install(args) =
            TestCli::parse_from(args).cmd
        else {
            panic!("not an install command");
        };
        args
    }

    fn parse_prune(args: &[&str]) -> RemoteSourcesPruneArgs {
        let RemoteSourcesSubcommand::Prune(args) =
            TestCli::parse_from(args).cmd
        else {
            panic!("not a prune command");
        };
        args
    }

    #[test]
    fn prune_flags_default_to_no_criteria() {
        let args = parse_prune(&["omni", "prune"]);
        assert!(!args.all && !args.yes && !args.dry_run);
        assert!(args.older_than.is_none() && args.larger_than.is_none());
    }

    #[test]
    fn prune_parses_age_size_and_modes() {
        let args = parse_prune(&[
            "omni",
            "prune",
            "--older-than",
            "30d",
            "--larger-than",
            "100MB",
            "--dry-run",
        ]);
        assert_eq!(args.older_than, Some(Duration::from_secs(30 * 86_400)));
        assert_eq!(args.larger_than, Some(ByteSize::mb(100)));
        assert!(args.dry_run);
    }

    #[test]
    fn prune_dry_run_conflicts_with_yes() {
        assert!(
            TestCli::try_parse_from(["omni", "prune", "--dry-run", "--yes"])
                .is_err()
        );
    }

    #[test]
    fn install_flags_default_to_false() {
        let args = parse(&["omni", "install"]);
        assert!(!args.update);
        assert!(!args.generators);
        assert!(!args.tools);
        assert!(!args.projections);
    }

    #[test]
    fn install_parses_every_flag() {
        let args = parse(&[
            "omni",
            "install",
            "--update",
            "--generators",
            "--tools",
            "--projections",
        ]);
        assert!(args.update);
        assert!(args.generators);
        assert!(args.tools);
        assert!(args.projections);
    }

    #[test]
    fn install_parses_a_single_subsystem_flag() {
        let args = parse(&["omni", "install", "--generators"]);
        assert!(args.generators);
        assert!(!args.tools);
        assert!(!args.projections);
    }
}
