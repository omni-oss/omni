use std::process::ExitCode;

use omni_api::{IgnoreCleanRequest, IgnoreSyncRequest, OmniApi};
use omni_context::Context;
use omni_messages::NoopSubscriber;
use owo_colors::OwoColorize;

#[derive(Debug, Clone, clap::Args)]
pub struct IgnoreCommand {
    #[command(subcommand)]
    pub subcommand: IgnoreSubcommand,
}

#[derive(Debug, Clone, clap::Subcommand)]
pub enum IgnoreSubcommand {
    #[command(
        about = "Patch omni's managed block into every configured ignore file"
    )]
    Sync(#[command(flatten)] IgnoreSyncArgs),

    #[command(
        about = "Remove omni's managed block from every configured ignore file"
    )]
    Clean,
}

#[derive(Debug, Clone, clap::Args)]
pub struct IgnoreSyncArgs {
    #[arg(
        long,
        help = "Print the block that would be written without touching any file",
        default_value_t = false,
        action = clap::ArgAction::SetTrue
    )]
    pub dry_run: bool,

    #[arg(
        long,
        help = "Exit non-zero if any file is missing the block or out of date; writes nothing",
        default_value_t = false,
        action = clap::ArgAction::SetTrue
    )]
    pub check: bool,
}

pub async fn run(cmd: &IgnoreCommand, ctx: &Context) -> eyre::Result<ExitCode> {
    match &cmd.subcommand {
        IgnoreSubcommand::Sync(args) => run_sync(args, ctx).await,
        IgnoreSubcommand::Clean => run_clean(ctx).await,
    }
}

async fn run_sync(
    args: &IgnoreSyncArgs,
    ctx: &Context,
) -> eyre::Result<ExitCode> {
    let response = OmniApi::new_with_sys(ctx.clone(), NoopSubscriber)
        .ignore_sync(IgnoreSyncRequest {
            dry_run: args.dry_run,
            check: args.check,
        })
        .await?;

    if response.dry_run {
        println!("{}", response.block);
        for file in &response.files {
            println!("  {} {}", file.status, file.path);
        }
        return Ok(ExitCode::SUCCESS);
    }

    for file in &response.files {
        println!("  {} {}", file.status, file.path);
    }

    if response.check {
        if response.up_to_date {
            println!("{}", "ignore files are up to date".green());
            Ok(ExitCode::SUCCESS)
        } else {
            println!(
                "{} one or more ignore files are missing the block or out of date",
                "error:".red()
            );
            Ok(ExitCode::FAILURE)
        }
    } else {
        Ok(ExitCode::SUCCESS)
    }
}

async fn run_clean(ctx: &Context) -> eyre::Result<ExitCode> {
    let response = OmniApi::new_with_sys(ctx.clone(), NoopSubscriber)
        .ignore_clean(IgnoreCleanRequest::default())
        .await?;

    for file in &response.files {
        println!("  {} {}", file.status, file.path);
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
        cmd: IgnoreSubcommand,
    }

    fn parse(args: &[&str]) -> IgnoreSubcommand {
        TestCli::parse_from(args).cmd
    }

    #[test]
    fn sync_flags_default_to_false() {
        let IgnoreSubcommand::Sync(args) = parse(&["omni", "sync"]) else {
            panic!("expected sync");
        };
        assert!(!args.dry_run);
        assert!(!args.check);
    }

    #[test]
    fn sync_parses_dry_run_and_check() {
        let IgnoreSubcommand::Sync(args) =
            parse(&["omni", "sync", "--dry-run", "--check"])
        else {
            panic!("expected sync");
        };
        assert!(args.dry_run);
        assert!(args.check);
    }

    #[test]
    fn clean_parses() {
        assert!(matches!(parse(&["omni", "clean"]), IgnoreSubcommand::Clean));
    }
}
