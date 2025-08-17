use crate::{
    commands::utils::report_execution_results,
    context::Context,
    executor::{Call, OnFailure, TaskOrchestrator},
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
    project: Option<String>,

    #[arg(
        short,
        long,
        help = "Filter the task/projects based on the meta configuration. Use the syntax of the CEL expression language"
    )]
    meta: Option<String>,

    #[arg(
        long,
        alias = "ignore-deps",
        short,
        help = "Run the command without dependencies",
        default_value_t = false
    )]
    ignore_dependencies: bool,

    #[arg(
        long,
        short,
        help = "How to handle failures",
        default_value_t = OnFailure::SkipDependents
    )]
    on_failure: OnFailure,

    #[arg(
        long,
        short,
        help = "Don't save the execution result to the cache",
        default_value_t = false
    )]
    no_cache: bool,

    #[arg(
        long,
        short,
        help = "Force execution of the task, even if it's already cached",
        default_value_t = false
    )]
    force: bool,
}

pub async fn run(command: &RunCommand, ctx: &Context) -> eyre::Result<()> {
    let mut builder = TaskOrchestrator::builder();

    builder
        .context(ctx.clone())
        .ignore_dependencies(command.ignore_dependencies)
        .on_failure(command.on_failure)
        .no_cache(command.no_cache)
        .force(command.force)
        .call(Call::new_task(&command.task));

    if let Some(filter) = &command.project {
        builder.project_filter(filter);
    }

    if let Some(filter) = &command.meta {
        builder.meta_filter(filter);
    }

    let orchestrator = builder.build()?;

    let results = orchestrator.execute().await?;

    report_execution_results(&results);

    Ok(())
}
