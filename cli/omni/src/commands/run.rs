use crate::{
    context::Context,
    executor::{Call, TaskOrchestrator},
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
        alias = "ignore-deps",
        short,
        help = "Run the command without dependencies",
        default_value_t = false
    )]
    ignore_dependencies: bool,

    #[arg(
        long,
        short,
        help = "Do not stop execution if dependencies fail",
        default_value_t = false
    )]
    ignore_failures: bool,

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
        .ignore_failures(command.ignore_failures)
        .no_cache(command.no_cache)
        .force(command.force)
        .call(Call::new_task(&command.task));

    if let Some(filter) = &command.filter {
        builder.project_filter(filter);
    }

    let orchestrator = builder.build()?;

    let results = orchestrator.execute().await?;

    trace::info!("Results: {:#?}", results);

    Ok(())
}
