use omni_api::{
    BackupHandling, OmniApi, ProjectionPruneRequest, ProjectionStatusRequest,
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

    #[arg(
        long,
        help = "Override the maximum meta-bundle nesting depth (default 16)"
    )]
    pub max_depth: Option<usize>,
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

    #[arg(
        long,
        help = "Restore any backups taken when the links were created to their original destinations",
        default_value_t = false,
        action = clap::ArgAction::SetTrue,
        conflicts_with = "clean_backups"
    )]
    pub restore_backups: bool,
}

impl ProjectionUnlinkArgs {
    fn backup_handling(&self) -> Option<BackupHandling> {
        if self.clean_backups {
            Some(BackupHandling::Clean)
        } else if self.restore_backups {
            Some(BackupHandling::Restore)
        } else {
            None
        }
    }
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
            max_depth: args.max_depth,
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
        let tree = build_status_tree(&response.entries);
        for line in render_status_tree(&tree, true) {
            println!("{line}");
        }
    }

    println!(
        "{} ok, {} missing, {} broken, {} drifted",
        response.ok, response.missing, response.broken, response.drifted
    );

    Ok(())
}

// ── Status tree rendering ────────────────────────────────────────────────────

/// A node in the status tree, keyed by one `::` segment of a composed id. An
/// internal node is a bundle; a node carrying `entries` is a link-bearing leaf
/// source.
#[derive(Default)]
struct StatusNode {
    children: std::collections::BTreeMap<String, StatusNode>,
    entries: Vec<(String, String)>,
}

#[derive(Default, Clone, Copy)]
struct StateCounts {
    ok: usize,
    missing: usize,
    broken: usize,
    drifted: usize,
}

impl StateCounts {
    fn add(&mut self, state: &str) {
        match state {
            "ok" => self.ok += 1,
            "missing" => self.missing += 1,
            "broken" => self.broken += 1,
            "drifted" => self.drifted += 1,
            _ => {}
        }
    }

    fn merge(&mut self, other: StateCounts) {
        self.ok += other.ok;
        self.missing += other.missing;
        self.broken += other.broken;
        self.drifted += other.drifted;
    }

    fn summary(&self) -> String {
        let mut parts = Vec::new();
        if self.ok > 0 {
            parts.push(format!("{} ok", self.ok));
        }
        if self.missing > 0 {
            parts.push(format!("{} missing", self.missing));
        }
        if self.broken > 0 {
            parts.push(format!("{} broken", self.broken));
        }
        if self.drifted > 0 {
            parts.push(format!("{} drifted", self.drifted));
        }
        format!("({})", parts.join(", "))
    }
}

/// Reconstruct the bundle tree purely from the flat status entries: split each
/// composed id on `::` and insert its link rows at the leaf segment.
fn build_status_tree(entries: &[omni_api::StatusEntryInfo]) -> StatusNode {
    let mut root = StatusNode::default();
    for entry in entries {
        let mut node = &mut root;
        for segment in entry.source_id.split("::") {
            node = node.children.entry(segment.to_string()).or_default();
        }
        node.entries.push((entry.state.clone(), entry.dest.clone()));
    }
    root
}

fn rollup(node: &StatusNode) -> StateCounts {
    let mut counts = StateCounts::default();
    for (state, _) in &node.entries {
        counts.add(state);
    }
    for child in node.children.values() {
        counts.merge(rollup(child));
    }
    counts
}

/// Render the tree to lines. `color` bolds bundle-group names (a node with
/// children); leaf sources show their local id and one row per link. Passing
/// `color = false` yields plain text for testing.
fn render_status_tree(root: &StatusNode, color: bool) -> Vec<String> {
    let mut out = Vec::new();
    render_level(root, 0, color, &mut out);
    out
}

fn render_level(
    node: &StatusNode,
    depth: usize,
    color: bool,
    out: &mut Vec<String>,
) {
    let indent = "  ".repeat(depth);
    for (segment, child) in &node.children {
        if child.children.is_empty() {
            // A leaf source: its local id followed by one row per link.
            out.push(format!("{indent}{segment}"));
            for (state, dest) in &child.entries {
                out.push(format!("{indent}  [{state}] {dest}"));
            }
        } else {
            // A bundle: a bold group header carrying a descendant rollup.
            let name = if color {
                segment.bold().to_string()
            } else {
                segment.clone()
            };
            out.push(format!("{indent}{name} {}", rollup(child).summary()));
            render_level(child, depth + 1, color, out);
            for (state, dest) in &child.entries {
                out.push(format!("{indent}  [{state}] {dest}"));
            }
        }
    }
}

async fn run_unlink(
    args: &ProjectionUnlinkArgs,
    ctx: &Context,
) -> eyre::Result<()> {
    let response = OmniApi::new_with_sys(ctx.clone(), NoopSubscriber)
        .projection_unlink(ProjectionUnlinkRequest {
            id: args.id.clone(),
            backup_handling: args.backup_handling(),
        })
        .await?;

    for removed in &response.removed {
        println!("  removed {removed}");
    }
    for restored in &response.restored {
        println!("  restored {restored}");
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
    use omni_api::{BackupHandling, StatusEntryInfo};

    use super::{ProjectionSubcommand, build_status_tree, render_status_tree};
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
                assert!(!args.restore_backups);
                assert_eq!(args.backup_handling(), Some(BackupHandling::Clean));
            }
            other => panic!("expected unlink, got {other:?}"),
        }
    }

    #[test]
    fn parses_unlink_with_restore_backups() {
        match projection_of(&[
            "omni",
            "projection",
            "unlink",
            "team-skills",
            "--restore-backups",
        ]) {
            ProjectionSubcommand::Unlink(args) => {
                assert!(args.restore_backups);
                assert_eq!(
                    args.backup_handling(),
                    Some(BackupHandling::Restore)
                );
            }
            other => panic!("expected unlink, got {other:?}"),
        }
    }

    #[test]
    fn unlink_without_backup_flags_leaves_backups() {
        match projection_of(&["omni", "projection", "unlink", "team-skills"]) {
            ProjectionSubcommand::Unlink(args) => {
                assert!(!args.clean_backups);
                assert!(!args.restore_backups);
                assert_eq!(args.backup_handling(), None);
            }
            other => panic!("expected unlink, got {other:?}"),
        }
    }

    #[test]
    fn unlink_clean_and_restore_backups_conflict() {
        let result = Cli::try_parse_from([
            "omni",
            "projection",
            "unlink",
            "team-skills",
            "--clean-backups",
            "--restore-backups",
        ]);
        assert!(result.is_err(), "the two flags must be mutually exclusive");
    }

    #[test]
    fn parses_prune_dry_run() {
        match projection_of(&["omni", "projection", "prune", "--dry-run"]) {
            ProjectionSubcommand::Prune(args) => assert!(args.dry_run),
            other => panic!("expected prune, got {other:?}"),
        }
    }

    #[test]
    fn parses_sync_max_depth() {
        match projection_of(&[
            "omni",
            "projection",
            "sync",
            "--max-depth",
            "32",
        ]) {
            ProjectionSubcommand::Sync(args) => {
                assert_eq!(args.max_depth, Some(32));
            }
            other => panic!("expected sync, got {other:?}"),
        }

        match projection_of(&["omni", "projection", "sync"]) {
            ProjectionSubcommand::Sync(args) => {
                assert_eq!(args.max_depth, None)
            }
            other => panic!("expected sync, got {other:?}"),
        }
    }

    fn entry(source_id: &str, dest: &str, state: &str) -> StatusEntryInfo {
        StatusEntryInfo {
            source_id: source_id.to_string(),
            dest: dest.to_string(),
            state: state.to_string(),
        }
    }

    #[test]
    fn status_tree_groups_by_segment_with_rollups() {
        let entries = vec![
            entry("org::rules", ".agents/rules", "drifted"),
            entry("org::skills", ".agents/skills", "ok"),
            entry("solo", ".agents/solo", "ok"),
        ];
        let tree = build_status_tree(&entries);
        let lines = render_status_tree(&tree, false);

        assert_eq!(
            lines,
            vec![
                "org (1 ok, 1 drifted)".to_string(),
                "  rules".to_string(),
                "    [drifted] .agents/rules".to_string(),
                "  skills".to_string(),
                "    [ok] .agents/skills".to_string(),
                "solo".to_string(),
                "  [ok] .agents/solo".to_string(),
            ]
        );
    }

    #[test]
    fn status_tree_renders_a_plain_source_as_a_flat_leaf() {
        let entries = vec![entry("skills-a", ".agents/a", "ok")];
        let tree = build_status_tree(&entries);
        let lines = render_status_tree(&tree, false);
        assert_eq!(
            lines,
            vec!["skills-a".to_string(), "  [ok] .agents/a".to_string()]
        );
    }

    #[test]
    fn status_tree_nests_one_indent_per_segment() {
        let entries = vec![entry("org::team::skills", ".agents/skills", "ok")];
        let tree = build_status_tree(&entries);
        let lines = render_status_tree(&tree, false);
        assert_eq!(
            lines,
            vec![
                "org (1 ok)".to_string(),
                "  team (1 ok)".to_string(),
                "    skills".to_string(),
                "      [ok] .agents/skills".to_string(),
            ]
        );
    }
}
