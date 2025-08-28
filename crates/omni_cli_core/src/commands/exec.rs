use std::process::ExitCode;

use clap::Args;

use crate::{
    commands::utils::report_execution_results,
    context::Context,
    executor::{Call, TaskExecutor},
};

#[derive(Args, Debug)]
pub struct ExecArgs {
    #[arg(
        long,
        short,
        help = "Run the command based on the project name matching the filter"
    )]
    project: Option<String>,
    #[arg(required = true)]
    command: String,
    #[arg(num_args(0..), help = "The arguments to pass to the task", trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
    #[arg(
        short,
        long,
        help = "Filter the task/projects based on the meta configuration. Use the syntax of the CEL expression language"
    )]
    meta: Option<String>,

    #[arg(long, short = 'c', help = "How many concurrent tasks to run")]
    max_concurrency: Option<usize>,
}

#[derive(Args)]
pub struct ExecCommand {
    #[command(flatten)]
    args: ExecArgs,
}

pub async fn run(
    command: &ExecCommand,
    ctx: &Context,
) -> eyre::Result<ExitCode> {
    let mut builder = TaskExecutor::builder();

    builder.context(ctx.clone()).call(Call::new_command(
        command.args.command.clone(),
        command.args.args.clone(),
    ));

    if let Some(filter) = &command.args.project {
        builder.project_filter(filter);
    }

    if let Some(filter) = &command.args.meta {
        builder.meta_filter(filter);
    }

    if let Some(max_concurrency) = command.args.max_concurrency {
        builder.max_concurrency(max_concurrency);
    }

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
