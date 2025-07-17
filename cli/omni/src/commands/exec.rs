use clap::Args;
use eyre::OptionExt as _;

use crate::{
    context::Context,
    utils::{self, dir_walker::create_default_dir_walker},
};

#[derive(Args, Debug)]
pub struct ExecArgs {
    #[arg(required = true)]
    command: String,
    #[arg(num_args(0..))]
    args: Vec<String>,
    #[arg(
        long,
        short,
        help = "Run the command based on the project name matching the filter"
    )]
    filter: Option<String>,
}

#[derive(Args)]
pub struct ExecCommand {
    #[command(flatten)]
    args: ExecArgs,
}

pub async fn run(command: &ExecCommand, ctx: &mut Context) -> eyre::Result<()> {
    ctx.load_projects(&create_default_dir_walker())?;
    let filter = if let Some(filter) = &command.args.filter {
        filter
    } else {
        "*"
    };
    let projects = ctx
        .get_filtered_projects(filter)?
        .iter()
        .map(|a| (*a).clone())
        .collect::<Vec<_>>();

    if projects.is_empty() {
        eyre::bail!("No project found for filter: {}", filter);
    }

    trace::debug!(
        "executing command: {} {:?}",
        command.args.command,
        command.args.args
    );
    trace::debug!("Projects: {:?}", projects);

    for p in projects {
        ctx.load_env_vars(
            p.dir.to_str().ok_or_eyre("Can't convert dir to str")?,
        )?;
        let envs = ctx.get_all_env_vars_os().clone();
        let exit_status = utils::cmd::run(
            &format!(
                "{} {}",
                command.args.command,
                command.args.args.join(" ")
            ),
            &p.dir,
            envs,
        )
        .await?;

        if exit_status != 0 {
            eyre::bail!("command exited with non-zero status: {}", exit_status);
        }
    }

    Ok(())
}
