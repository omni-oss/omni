use std::{collections::HashMap, sync::Arc};

use futures::future::join_all;
use tokio::sync::Mutex;

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
    #[arg(
        long,
        short,
        help = "Run the command ignoring the dependencies of each project"
    )]
    ignore_dependencies: bool,
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

    trace::debug!("Projects: {:?}", projects);

    let matcher = ctx.get_filter_glob_matcher(filter)?;

    let execution_plan = ctx
        .get_project_graph()?
        .get_task_execution_graph()?
        .get_batched_execution_plan(|n| {
        matcher.is_match(n.project_name())
    })?;

    if execution_plan.is_empty() {
        eyre::bail!("No tasks were executed: {}", command.task);
    }

    let shared_ctx = Arc::new(Mutex::new(ctx.clone()));
    // cache envs for each project dir so that we don't have to load them again
    let project_dir_envs = Arc::new(Mutex::new(HashMap::new()));
    let failures = Arc::new(Mutex::new(Vec::new()));

    trace::debug!("Execution plan: {:?}", execution_plan);
    for batch in execution_plan {
        let mut tasks = vec![];

        for task in batch {
            let ctx = shared_ctx.clone();
            let dir_envs = project_dir_envs.clone();
            let failures = failures.clone();

            tasks.push(async move {
                let project_dir_str = task
                    .project_dir()
                    .to_str()
                    .expect("Can't convert project dir to str");

                let envs = {
                    // Scope the lock to the duration of the task so that we don't hold the lock for the entire duration of the task
                    let mut hm = dir_envs.lock().await;

                    if !hm.contains_key(project_dir_str) {
                        let (_, vars_os) =
                            ctx.lock().await.get_env_vars_at_start_dir(
                                task.project_dir()
                                    .to_str()
                                    .expect("Can't convert project dir to str"),
                            )?;

                        hm.insert(project_dir_str.to_string(), vars_os);
                    }

                    hm.get(project_dir_str)
                        .ok_or_else(|| {
                            eyre::eyre!("Should be in map at this point")
                        })?
                        .clone()
                };

                trace::info!(
                    "Running task '{}#{}'",
                    task.project_name(),
                    task.task_name()
                );

                let exit = utils::cmd::run(
                    task.task_command(),
                    task.project_dir(),
                    envs,
                )
                .await?;

                if exit != 0 {
                    failures.lock().await.push((task, exit));
                }

                Ok::<_, eyre::Report>(())
            });
        }

        // run all tasks concurrently
        join_all(tasks).await;

        let f = failures.lock().await;
        if !f.is_empty() {
            for (task, exit_status) in f.iter() {
                trace::error!(
                    "Task '{}#{}' failed with exit code: {}",
                    task.project_name(),
                    task.task_name(),
                    exit_status
                );
            }
        }
    }

    Ok(())
}
