use std::process::Stdio;

use eyre::{Context as _, OptionExt};
use tokio::process;

use crate::context::Context;

#[derive(clap::Args)]
pub struct RunCommand {
    #[arg(required = true, help = "The task to run")]
    task: String,
    #[arg(num_args(0..), help = "The arguments to pass to the task")]
    args: Vec<String>,
    #[arg(
        long,
        short,
        help = "Run the command based on the project name matching the filter"
    )]
    filter: Option<String>,
}

pub async fn run(command: &RunCommand, ctx: &mut Context) -> eyre::Result<()> {
    if command.task.is_empty() {
        eyre::bail!("Task cannot be empty");
    }

    ctx.load_projects()?;
    let filter = if let Some(filter) = &command.filter {
        filter
    } else {
        "*"
    };

    let projects = ctx.get_filtered_projects(filter)?;

    if projects.is_empty() {
        tracing::error!("No project found for filter: {}", filter);
        return Ok(());
    }

    let projects = projects.iter().map(|a| (*a).clone()).collect::<Vec<_>>();

    tracing::debug!("Projects: {:?}", projects);

    let mut task_executed = 0;
    for p in projects {
        ctx.load_env_vars(
            p.dir.to_str().ok_or_eyre("Can't convert dir to str")?,
        )?;

        if let Some(task) = p.tasks.get(&command.task) {
            task_executed += 1;

            if task.command.is_empty() {
                tracing::warn!("Task {} has no command", command.task);
                continue;
            } else {
                tracing::debug!(
                    "Executing task: {} => {}",
                    command.task,
                    task.command
                );
            }

            let split = task.command.split_whitespace().collect::<Vec<_>>();

            let args = split[1..]
                .iter()
                .copied()
                .chain(command.args.iter().map(|s| s.as_str()))
                .collect::<Vec<_>>();

            let prog = split[0];

            let exit_status = process::Command::new(prog)
                .args(&args)
                .envs(ctx.get_all_env_vars())
                .current_dir(&p.dir)
                .stderr(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stdin(Stdio::inherit())
                .spawn()
                .wrap_err_with(|| {
                    format!("failed to execute command: {} {:?}", prog, args)
                })?
                .wait()
                .await?;

            if !exit_status.success() {
                eyre::bail!(
                    "command exited with non-zero status: {}",
                    exit_status
                );
            }
        }
    }

    if task_executed == 0 {
        tracing::warn!("No task found for: {}", command.task);
    }

    Ok(())
}
