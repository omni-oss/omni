use omni_api::{
    OmniApi, PackInfoRequest, PackListRequest, PackListResponse,
    PackTreeRequest,
};
use omni_context::Context;
use omni_messages::NoopSubscriber;
use owo_colors::OwoColorize;

#[derive(Debug, Clone, clap::Args)]
pub struct PackCommand {
    #[command(subcommand)]
    pub subcommand: PackSubcommand,
}

#[derive(Debug, Clone, clap::Subcommand)]
pub enum PackSubcommand {
    #[command(about = "List the top-level packs declared in the workspace")]
    List,

    #[command(about = "Print the expanded pack-of-packs graph")]
    Tree,

    #[command(about = "Show one pack and its subtree by qualified id")]
    Info(PackInfoArgs),
}

#[derive(Debug, Clone, clap::Args)]
pub struct PackInfoArgs {
    #[arg(help = "The qualified id of the pack (or subtree) to inspect")]
    pub qualified_id: String,
}

pub async fn run(cmd: &PackCommand, ctx: &Context) -> eyre::Result<()> {
    match &cmd.subcommand {
        PackSubcommand::List => run_list(ctx).await,
        PackSubcommand::Tree => run_tree(ctx).await,
        PackSubcommand::Info(args) => run_info(args, ctx).await,
    }
}

async fn run_list(ctx: &Context) -> eyre::Result<()> {
    let response = OmniApi::new_with_sys(ctx.clone(), NoopSubscriber)
        .pack_list(PackListRequest)
        .await?;
    print_packs(&response, false);
    Ok(())
}

async fn run_tree(ctx: &Context) -> eyre::Result<()> {
    let response = OmniApi::new_with_sys(ctx.clone(), NoopSubscriber)
        .pack_tree(PackTreeRequest)
        .await?;
    print_packs(&response, true);
    Ok(())
}

async fn run_info(args: &PackInfoArgs, ctx: &Context) -> eyre::Result<()> {
    let response = OmniApi::new_with_sys(ctx.clone(), NoopSubscriber)
        .pack_info(PackInfoRequest {
            qualified_id: args.qualified_id.clone(),
        })
        .await?;
    print_packs(&response, true);
    Ok(())
}

fn print_packs(response: &PackListResponse, show_depth: bool) {
    if response.packs.is_empty() {
        println!("No packs configured.");
        return;
    }

    for pack in &response.packs {
        let indent = if show_depth {
            "  ".repeat(pack.qualified_id.matches("::").count())
        } else {
            String::new()
        };

        let version = pack
            .version
            .as_deref()
            .map(|v| format!(" @{v}"))
            .unwrap_or_default();

        println!(
            "{indent}{} {}{}",
            pack.qualified_id.bold(),
            pack.name.dimmed(),
            version.dimmed(),
        );

        if let Some(pin) = &pack.pin {
            println!("{indent}  pin: {}", pin.dimmed());
        }
        if !pack.provides.is_empty() {
            println!("{indent}  provides: {}", pack.provides.join(", "));
        }
    }
}
