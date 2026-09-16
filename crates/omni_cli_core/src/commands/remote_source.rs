use std::process::ExitCode;

use omni_api::{OmniApi, RemoteSourcesInstallRequest, SubsystemSelection};
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
    println!("  garbage-collected: {}", response.garbage_collected);

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
            TestCli::parse_from(args).cmd;
        args
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
