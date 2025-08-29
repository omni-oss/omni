use std::process::ExitCode;

use clap::Args;

use crate::{
    commands::{
        common_args::RunArgs,
        utils::{
            exit_code, get_results_settings, report_execution_results,
            write_results,
        },
    },
    context::Context,
    executor::{Call, TaskExecutor},
};

#[derive(Args, Debug)]
pub struct ExecArgs {
    #[arg(required = true)]
    command: String,
    #[arg(num_args(0..), help = "The arguments to pass to the task", trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,

    #[command(flatten)]
    run: RunArgs,
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
    let output_settings = get_results_settings(&command.args.run)?;

    let mut builder = TaskExecutor::builder();

    if output_settings.is_some() {
        builder.add_task_details(true);
    }

    builder.context(ctx.clone()).call(Call::new_command(
        command.args.command.clone(),
        command.args.args.clone(),
    ));

    command.args.run.apply_to(&mut builder);

    let orchestrator = builder.build()?;

    let results = orchestrator.execute().await?;

    report_execution_results(&results);

    if let Some((fmt, results_file_path)) = output_settings {
        write_results(&results, fmt, results_file_path)?;
    }

    Ok(exit_code(&results))
}
