use eyre::OptionExt;

use crate::{
    context::Context,
    utils::{self, dir_walker::create_default_dir_walker},
};

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

    ctx.load_projects(&create_default_dir_walker())?;
    let filter = if let Some(filter) = &command.filter {
        filter
    } else {
        "*"
    };

    let projects = ctx.get_filtered_projects(filter)?;

    if projects.is_empty() {
        eyre::bail!("No project found for filter: {}", filter);
    }

    let projects = projects.iter().map(|a| (*a).clone()).collect::<Vec<_>>();

    trace::debug!("Projects: {:?}", projects);

    let mut task_executed = 0;
    for p in projects {
        ctx.load_env_vars(
            p.dir.to_str().ok_or_eyre("Can't convert dir to str")?,
        )?;

        if let Some(task) = p.tasks.get(&command.task) {
            task_executed += 1;

            if task.command.is_empty() {
                trace::warn!("Task {} has no command", command.task);
                continue;
            } else {
                trace::debug!(
                    "Executing task: {} => {}",
                    command.task,
                    task.command
                );
            }

            let exit_status = utils::cmd::run(
                &task.command,
                &p.dir,
                ctx.get_all_env_vars_os(),
            )
            .await?;

            if exit_status != 0 {
                eyre::bail!(
                    "command exited with non-zero status: {}",
                    exit_status
                );
            }
        }
    }

    if task_executed == 0 {
        eyre::bail!("No tasks were executed: {}", command.task);
    }

    Ok(())
}
