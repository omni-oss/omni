use std::process::ExitCode;

use clap::Args;
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
    executor::{Call, TaskExecutor},
};

#[derive(Args, Debug)]
pub struct ExecCommand {
    #[arg(required = true)]
    pub command: String,

    #[command(flatten)]
    pub run: RunArgs,

    #[arg(num_args(0..), help = "The arguments to pass to the task", trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

pub async fn run(
    command: &ExecCommand,
    ctx: &Context,
) -> eyre::Result<ExitCode> {
    let output_settings = get_results_settings(&command.run)?;

    let mut builder = ExecutionConfigBuilder::default();

    if output_settings.is_some() {
        builder.add_task_details(true);
    }

    builder.call(Call::new_command(
        command.command.clone(),
        command.args.clone(),
    ));

    command
        .run
        .apply_to(&mut builder, ctx.workspace_configuration());

    let config = builder.build()?;

    let ctx = ctx.clone().into_loaded().await?;
    let executor = TaskExecutor::new(config, &ctx);

    let results = executor.run().await?;

    report_execution_results(&results);

    if let Some((fmt, results_file_path)) = output_settings {
        write_results(&results, fmt, results_file_path)?;
    }

    Ok(exit_code(&results))
}
