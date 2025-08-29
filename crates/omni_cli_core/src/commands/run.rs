use std::process::ExitCode;

use crate::{
    commands::{common_args::RunArgs, utils::report_execution_results},
    context::Context,
    executor::{Call, OnFailure, TaskExecutor},
};

#[derive(clap::Args)]
pub struct RunCommand {
    #[arg(required = true, help = "The task to run")]
    task: String,

    #[arg(num_args(0..), help = "The arguments to pass to the task")]
    args: Vec<String>,

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
        help = "Don't save the execution result to the cache",
        default_value_t = false
    )]
    no_cache: bool,

    #[arg(
        long,
        short = 'L',
        help = "Don't replay the logs of cached task executions"
    )]
    no_replay_logs: bool,

    #[arg(
        long,
        short,
        help = "Force execution of the task, even if it's already cached",
        default_value_t = false
    )]
    force: bool,

    #[command(flatten)]
    run: RunArgs,
}

pub async fn run(
    command: &RunCommand,
    ctx: &Context,
) -> eyre::Result<ExitCode> {
    let mut builder = TaskExecutor::builder();

    builder
        .context(ctx.clone())
        .ignore_dependencies(command.ignore_dependencies)
        .on_failure(command.on_failure)
        .no_cache(command.no_cache)
        .force(command.force)
        .replay_cached_logs(!command.no_replay_logs)
        .call(Call::new_task(&command.task));

    command.run.apply_to(&mut builder);

    let orchestrator = builder.build()?;

    let results = orchestrator.execute().await?;

    report_execution_results(&results);

    let has_error = results.iter().any(|r| r.skipped_or_error());

    Ok(if has_error {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}
