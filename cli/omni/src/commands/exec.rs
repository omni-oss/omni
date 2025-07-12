use std::process::Stdio;

use clap::Args;
use eyre::Context as _;
use tokio::process;

use crate::{context::Context, utils::dir_walker::create_default_dir_walker};

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
    let vars = ctx.get_all_env_vars();
    let filter = if let Some(filter) = &command.args.filter {
        filter
    } else {
        "*"
    };
    let projects = ctx.get_filtered_projects(filter)?;

    if projects.is_empty() {
        eyre::bail!("No project found for filter: {}", filter);
    }

    let mut cmd = process::Command::new(&command.args.command);

    tracing::debug!(
        "executing command: {} {:?}",
        command.args.command,
        command.args.args
    );
    tracing::debug!("Projects: {:?}", projects);

    for p in projects {
        let exit = cmd
            .args(&command.args.args)
            .current_dir(&p.dir)
            .envs(vars)
            .stderr(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stdin(Stdio::inherit())
            .spawn()
            .wrap_err_with(|| {
                format!(
                    "failed to execute command: {} {}",
                    command.args.command,
                    command.args.args.join(" ")
                )
            })?
            .wait()
            .await?;

        if !exit.success() {
            eyre::bail!("command exited with non-zero status: {}", exit);
        }
    }

    Ok(())
}
