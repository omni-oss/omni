use std::process::ExitCode;

use clap_utils::EnumValueAdapter;
use omni_task_executor::ExecutionConfigBuilder;

use crate::{
    commands::{
        common_args::RunArgs,
        utils::{
            exit_code, get_results_settings, report_execution_results,
            write_results,
        },
    },
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
        default_value_t = EnumValueAdapter::new(OnFailure::SkipDependents),
        value_enum
    )]
    on_failure: EnumValueAdapter<OnFailure>,

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
    let output_settings = get_results_settings(&command.run)?;

    let mut builder = ExecutionConfigBuilder::default();

    if output_settings.is_some() {
        builder.add_task_details(true);
    }

    builder
        .ignore_dependencies(command.ignore_dependencies)
        .on_failure(command.on_failure.value())
        .no_cache(command.no_cache)
        .force(command.force)
        .replay_cached_logs(!command.no_replay_logs)
        .call(Call::new_task(&command.task));

    command.run.apply_to(&mut builder);

    let config = builder.build()?;

    let ctx = ctx.clone().into_loaded().await?;
    let executor = TaskExecutor::new(config, &ctx);

    let results = executor.execute().await?;

    report_execution_results(&results);

    if let Some((fmt, results_file_path)) = output_settings {
        write_results(&results, fmt, results_file_path)?;
    }

    Ok(exit_code(&results))
}
