use std::process::ExitCode;

use std::time::Duration;

use bytesize::ByteSize;
use omni_api::{
    OmniApi, RemoteSourcesInstallRequest, RemoteSourcesPruneRequest,
    StoreCheckoutId, SubsystemSelection,
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
        about = "Delete checkouts from the remote-source store. They are refetched when next used",
        long_about = "Delete checkouts from the remote-source store. They are refetched when next used.\n\nCheckouts leased by a running command, or referenced by a registered workspace, are skipped and reported. Workspaces register when they write references (install, or a generator or tool run). An older omni version prunes without reading registrations, so it can delete a checkout a newer workspace still references."
    )]
    Prune(#[command(flatten)] RemoteSourcesPruneArgs),
}

#[derive(Debug, Clone, clap::Args)]
pub struct RemoteSourcesPruneArgs {
    #[arg(
        long,
        action = clap::ArgAction::SetTrue,
        conflicts_with_all = ["all", "older_than", "larger_than"],
        help = "Select checkouts that no running command holds and no registered workspace references. Workspaces register when they write references (install, or a generator or tool run)."
    )]
    pub unused: bool,

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
        conflicts_with_all = ["older_than", "larger_than"],
        help = "Select every checkout in the store (not combinable with --older-than or --larger-than)"
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
    let request = |dry_run, only| RemoteSourcesPruneRequest {
        all: args.all,
        unused: args.unused,
        older_than: args.older_than,
        larger_than: args.larger_than,
        dry_run,
        only,
    };

    let candidates = api.remote_sources_prune(request(true, None)).await?;
    for checkout in &candidates.skipped_in_use {
        println!(
            "  {} {} {} is in use by a running process and will be skipped",
            checkout.kind, checkout.remote, checkout.pin
        );
    }
    for checkout in &candidates.skipped_referenced {
        println!(
            "  {} {} {} is referenced by a registered workspace and will be skipped",
            checkout.kind, checkout.remote, checkout.pin
        );
    }
    // A checkout that failed the preview stays on disk and is not offered for
    // deletion, so it fails the command on every path below.
    report_failures("Cannot prune", &candidates.failed);
    let failed_status = if candidates.failed.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    };
    if candidates.checkouts.is_empty() {
        if candidates.skipped_in_use.is_empty()
            && candidates.skipped_referenced.is_empty()
            && candidates.failed.is_empty()
        {
            println!("No store checkouts matched the given filters");
        } else {
            println!(
                "Nothing to prune: every matching checkout is in use, referenced or failed"
            );
        }
        return Ok(failed_status);
    }
    let total: u64 = candidates.checkouts.iter().map(|c| c.size_bytes).sum();
    for checkout in &candidates.checkouts {
        println!(
            "  {} {} {} ({}, unused for {})",
            checkout.kind,
            checkout.remote,
            checkout.pin,
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
        return Ok(failed_status);
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
            return Ok(failed_status);
        }
    }

    // Delete exactly what was listed (and confirmed), not whatever matches by
    // the time the user answers.
    let shown = candidates
        .checkouts
        .iter()
        .map(|c| StoreCheckoutId {
            kind: c.kind.clone(),
            remote: c.remote.clone(),
            pin: c.pin.clone(),
        })
        .collect();
    let pruned = api
        .remote_sources_prune(request(false, Some(shown)))
        .await?;
    println!("Pruned {} checkouts", pruned.checkouts.len());
    for checkout in &pruned.skipped_in_use {
        println!(
            "Skipped {} {} {}: in use by a running process",
            checkout.kind, checkout.remote, checkout.pin
        );
    }
    for checkout in &pruned.skipped_referenced {
        println!(
            "Skipped {} {} {}: referenced by a registered workspace",
            checkout.kind, checkout.remote, checkout.pin
        );
    }
    // Report what was removed first, then fail so a script notices checkouts
    // that are still on disk.
    report_failures("Could not prune", &pruned.failed);
    if pruned.failed.is_empty() && candidates.failed.is_empty() {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::FAILURE)
    }
}

fn report_failures(prefix: &str, failed: &[omni_api::StoreCheckoutFailure]) {
    for failure in failed {
        eprintln!(
            "{prefix} {} {} {}: {}",
            failure.checkout.kind,
            failure.checkout.remote,
            failure.checkout.pin,
            failure.error
        );
    }
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
        assert!(!args.all && !args.unused && !args.yes && !args.dry_run);
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
    fn prune_all_cannot_be_combined_with_age_or_size_filters() {
        for extra in [["--older-than", "30d"], ["--larger-than", "1MB"]] {
            let mut argv = vec!["omni", "prune", "--all"];
            argv.extend(extra);
            assert!(TestCli::try_parse_from(argv).is_err(), "{extra:?}");
        }
    }

    #[test]
    fn prune_unused_conflicts_with_every_narrowing_filter() {
        for extra in [
            &["--all"][..],
            &["--older-than", "30d"],
            &["--larger-than", "1MB"],
        ] {
            let mut argv = vec!["omni", "prune", "--unused"];
            argv.extend(extra);
            assert!(TestCli::try_parse_from(argv).is_err(), "{extra:?}");
        }
        let args = parse_prune(&["omni", "prune", "--unused", "--yes"]);
        assert!(args.unused && args.yes);
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
