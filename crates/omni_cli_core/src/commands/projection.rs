use omni_api::{
    OmniApi, ProjectionPruneRequest, ProjectionStatusRequest,
    ProjectionSyncRequest, ProjectionUnlinkRequest,
};
use omni_context::Context;
use omni_messages::NoopSubscriber;
use owo_colors::OwoColorize;

#[derive(Debug, Clone, clap::Args)]
pub struct ProjectionCommand {
    #[command(subcommand)]
    pub subcommand: ProjectionSubcommand,
}

#[derive(Debug, Clone, clap::Subcommand)]
pub enum ProjectionSubcommand {
    #[command(about = "Materialize configured projections into the workspace")]
    Sync(#[command(flatten)] ProjectionSyncArgs),

    #[command(about = "Report the state of recorded projection links")]
    Status(#[command(flatten)] ProjectionStatusArgs),

    #[command(about = "Remove the links recorded for a projection source")]
    Unlink(#[command(flatten)] ProjectionUnlinkArgs),

    #[command(about = "Remove links whose destinations have become dangling")]
    Prune(#[command(flatten)] ProjectionPruneArgs),
}

#[derive(Debug, Clone, clap::Args)]
pub struct ProjectionSyncArgs {
    #[arg(
        long,
        help = "Compute the plan without touching the filesystem",
        default_value_t = false,
        action = clap::ArgAction::SetTrue
    )]
    pub dry_run: bool,

    #[arg(
        long,
        help = "Re-apply and repair every link even when its pin is unchanged",
        default_value_t = false,
        action = clap::ArgAction::SetTrue
    )]
    pub force: bool,

    #[arg(
        long,
        help = "Re-resolve mutable git revisions (e.g. branches) before applying",
        default_value_t = false,
        action = clap::ArgAction::SetTrue
    )]
    pub update: bool,

    #[arg(long, help = "Limit the pass to the projection source with this id")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, clap::Args)]
pub struct ProjectionStatusArgs {
    #[arg(
        long,
        short = 'v',
        help = "List every recorded link, not just a summary",
        default_value_t = false,
        action = clap::ArgAction::SetTrue
    )]
    pub verbose: bool,
}

#[derive(Debug, Clone, clap::Args)]
pub struct ProjectionUnlinkArgs {
    #[arg(help = "The id of the projection source to tear down")]
    pub id: String,

    #[arg(
        long,
        help = "Also remove any backups taken when the links were created",
        default_value_t = false,
        action = clap::ArgAction::SetTrue
    )]
    pub clean_backups: bool,
}

#[derive(Debug, Clone, clap::Args)]
pub struct ProjectionPruneArgs {
    #[arg(
        long,
        help = "Report what would be pruned without removing anything",
        default_value_t = false,
        action = clap::ArgAction::SetTrue
    )]
    pub dry_run: bool,
}

pub async fn run(cmd: &ProjectionCommand, ctx: &Context) -> eyre::Result<()> {
    match &cmd.subcommand {
        ProjectionSubcommand::Sync(args) => run_sync(args, ctx).await,
        ProjectionSubcommand::Status(args) => run_status(args, ctx).await,
        ProjectionSubcommand::Unlink(args) => run_unlink(args, ctx).await,
        ProjectionSubcommand::Prune(args) => run_prune(args, ctx).await,
    }
}

async fn run_sync(
    args: &ProjectionSyncArgs,
    ctx: &Context,
) -> eyre::Result<()> {
    let response = OmniApi::new_with_sys(ctx.clone(), NoopSubscriber)
        .projection_sync(ProjectionSyncRequest {
            dry_run: args.dry_run,
            force: args.force,
            update: args.update,
            source: args.source.clone(),
        })
        .await?;

    if response.dry_run {
        println!("{}", "Planned links (dry run):".bold());
        for link in &response.planned {
            println!("  {} -> {}", link.dest, link.target);
        }
        println!("{} link(s) would be materialized", response.planned.len());
        return Ok(());
    }

    for link in &response.applied {
        let verb = if link.skipped { "up-to-date" } else { "linked" };
        println!("  {} {} ({})", verb, link.dest, link.kind);
    }
    for removed in &response.removed {
        println!("  removed {removed}");
    }
    for warning in &response.warnings {
        println!("  {} {}", "warning:".yellow(), warning);
    }
    println!(
        "{} link(s) applied, {} removed",
        response.applied.len(),
        response.removed.len()
    );

    Ok(())
}

async fn run_status(
    args: &ProjectionStatusArgs,
    ctx: &Context,
) -> eyre::Result<()> {
    let response = OmniApi::new_with_sys(ctx.clone(), NoopSubscriber)
        .projection_status(ProjectionStatusRequest {
            verbose: args.verbose,
        })
        .await?;

    if args.verbose {
        for entry in &response.entries {
            println!(
                "  [{}] {} ({})",
                entry.state, entry.dest, entry.source_id
            );
        }
    }

    println!(
        "{} ok, {} missing, {} broken, {} drifted",
        response.ok, response.missing, response.broken, response.drifted
    );

    Ok(())
}

async fn run_unlink(
    args: &ProjectionUnlinkArgs,
    ctx: &Context,
) -> eyre::Result<()> {
    let response = OmniApi::new_with_sys(ctx.clone(), NoopSubscriber)
        .projection_unlink(ProjectionUnlinkRequest {
            id: args.id.clone(),
            clean_backups: args.clean_backups,
        })
        .await?;

    for removed in &response.removed {
        println!("  removed {removed}");
    }
    for warning in &response.warnings {
        println!("  {} {}", "warning:".yellow(), warning);
    }
    println!("{} link(s) removed", response.removed.len());

    Ok(())
}

async fn run_prune(
    args: &ProjectionPruneArgs,
    ctx: &Context,
) -> eyre::Result<()> {
    let response = OmniApi::new_with_sys(ctx.clone(), NoopSubscriber)
        .projection_prune(ProjectionPruneRequest {
            dry_run: args.dry_run,
        })
        .await?;

    let verb = if response.dry_run {
        "would remove"
    } else {
        "removed"
    };
    for removed in &response.removed {
        println!("  {verb} {removed}");
    }
    println!("{} dangling link(s) {verb}", response.removed.len());

    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::ProjectionSubcommand;
    use crate::commands::{Cli, CliSubcommands};

    fn projection_of(args: &[&str]) -> ProjectionSubcommand {
        let cli = Cli::try_parse_from(args).expect("should parse");
        match cli.subcommand {
            CliSubcommands::Projection(cmd) => cmd.subcommand,
            _ => panic!("expected projection subcommand"),
        }
    }

    #[test]
    fn cli_definition_is_valid() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_sync_with_all_flags() {
        match projection_of(&[
            "omni",
            "projection",
            "sync",
            "--dry-run",
            "--force",
            "--update",
            "--source",
            "team-skills",
        ]) {
            ProjectionSubcommand::Sync(args) => {
                assert!(args.dry_run);
                assert!(args.force);
                assert!(args.update);
                assert_eq!(args.source.as_deref(), Some("team-skills"));
            }
            other => panic!("expected sync, got {other:?}"),
        }
    }

    #[test]
    fn sync_flags_default_to_false() {
        match projection_of(&["omni", "projection", "sync"]) {
            ProjectionSubcommand::Sync(args) => {
                assert!(!args.dry_run);
                assert!(!args.force);
                assert!(!args.update);
                assert!(args.source.is_none());
            }
            other => panic!("expected sync, got {other:?}"),
        }
    }

    #[test]
    fn parses_status_verbose() {
        match projection_of(&["omni", "projection", "status", "-v"]) {
            ProjectionSubcommand::Status(args) => assert!(args.verbose),
            other => panic!("expected status, got {other:?}"),
        }
    }

    #[test]
    fn parses_unlink_with_id_and_clean_backups() {
        match projection_of(&[
            "omni",
            "projection",
            "unlink",
            "team-skills",
            "--clean-backups",
        ]) {
            ProjectionSubcommand::Unlink(args) => {
                assert_eq!(args.id, "team-skills");
                assert!(args.clean_backups);
            }
            other => panic!("expected unlink, got {other:?}"),
        }
    }

    #[test]
    fn parses_prune_dry_run() {
        match projection_of(&["omni", "projection", "prune", "--dry-run"]) {
            ProjectionSubcommand::Prune(args) => assert!(args.dry_run),
            other => panic!("expected prune, got {other:?}"),
        }
    }
}
