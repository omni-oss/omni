use std::sync::Arc;

use futures::future::join_all;
use omni_core::TaskExecutionNode;

use crate::{
    commands::utils::execute_task, context::Context,
    utils::dir_walker::create_default_dir_walker,
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
        alias = "no-deps",
        short,
        help = "Run the command without dependencies",
        default_value_t = false
    )]
    no_dependencies: bool,
    #[arg(
        long,
        short,
        help = "Do not stop execution if dependencies fail",
        default_value_t = false
    )]
    ignore_failures: bool,
}

pub async fn run(command: &RunCommand, ctx: &Context) -> eyre::Result<()> {
    if command.task.is_empty() {
        eyre::bail!("Task cannot be empty");
    }
    let mut ctx = ctx.clone();
    ctx.load_projects(&create_default_dir_walker())?;

    let filter = command.filter.as_deref().unwrap_or("*");

    let ctx = Arc::new(ctx);

    let mut failures = vec![];

    if command.no_dependencies {
        let projects = ctx.get_filtered_projects(filter)?;
        let mut tasks = vec![];
        trace::debug!("Projects: {:#?}", projects);

        if projects.is_empty() {
            trace::error!(
                "No projects found matching filter '{}'. Nothing to run.",
                filter
            );
            eyre::bail!(
                "No projects found matching filter '{}'. Nothing to run.",
                filter
            );
        }

        for project in projects {
            let task = if let Some(task) = project.tasks.get(&command.task) {
                task
            } else {
                // skip tasks that don't exist
                continue;
            };

            let task = TaskExecutionNode::new(
                command.task.clone(),
                task.command.clone(),
                project.name.clone(),
                project.dir.clone(),
            );

            tasks.push(execute_task(task, ctx.clone()));
        }

        let results = join_all(tasks).await;
        let f = results
            .into_iter()
            .filter_map(|r| r.err())
            .collect::<Vec<_>>();

        failures.extend(f);
    } else {
        let project_name_matcher = ctx.get_filter_matcher(filter)?;

        let execution_plan = ctx
            .get_project_graph()?
            .get_task_execution_graph()?
            .get_batched_execution_plan(|n| {
                project_name_matcher.is_match(n.project_name())
                    && n.task_name() == command.task
            })?;

        if execution_plan.is_empty() {
            trace::error!(
                "No projects found matching filter '{}'. Nothing to run.",
                filter
            );
            eyre::bail!(
                "No projects found matching filter '{}'. Nothing to run.",
                filter
            );
        }

        trace::debug!("Execution plan: {:#?}", execution_plan);
        for batch in execution_plan {
            let mut tasks = vec![];

            for task in batch {
                tasks.push(execute_task(task, ctx.clone()));
            }
            // run all tasks in a batch concurrently
            let results = join_all(tasks).await;
            let f = results
                .into_iter()
                .filter_map(|r| r.err())
                .collect::<Vec<_>>();

            if !f.is_empty() && !command.ignore_failures {
                break;
            }

            failures.extend(f);
        }
    }

    if !failures.is_empty() {
        trace::error!("Failed to execute tasks: {:#?}", failures);

        let reports = failures.into_iter().map(|f| f.1).collect::<Vec<_>>();

        eyre::bail!("Failed to execute tasks: {:#?}", reports);
    }

    Ok(())
}
