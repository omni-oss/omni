use std::process::ExitCode;

use clap::Args;
use omni_task_executor::ExecutionConfigBuilder;

use crate::{
    commands::{
        common_args::RunArgs,
        utils::{exit_code, get_results_settings},
    },
    context::Context,
    executor::{Call, TaskExecutor},
};

use super::utils::resolve_subscriber;

#[derive(Args, Debug)]
pub struct ExecCommand {
    #[command(flatten)]
    pub run: RunArgs,

    #[arg(num_args(1..), help = "The command to run", trailing_var_arg = true, allow_hyphen_values = true)]
    pub cmd: Vec<String>,
}

pub async fn run(
    command: &ExecCommand,
    ctx: &Context,
) -> eyre::Result<ExitCode> {
    if command.cmd.is_empty() {
        eyre::bail!(
            "no command provided to exec; pass a command after `--`, e.g. `omni exec -- echo hello`"
        );
    }

    let output_settings = get_results_settings(&command.run)?;

    let mut builder = ExecutionConfigBuilder::default();

    if output_settings.is_some() {
        builder.add_task_details(true);
    }

    builder.call(Call::new_command(
        command.cmd[0].clone(),
        if command.cmd.len() > 1 {
            command.cmd[1..].iter().cloned().collect::<Vec<_>>()
        } else {
            vec![]
        },
    ));

    command
        .run
        .apply_to(&mut builder, ctx.workspace_configuration());

    let config = builder.build()?;

    let ctx = ctx.clone().into_loaded().await?;
    let sub = resolve_subscriber(command.run.ui, ctx.scratch_dir());
    let executor = TaskExecutor::new(config, &ctx, &sub);

    let results = executor.run().await?;

    sub.wait().await;

    // report_execution_results is now handled by CliSubscriber::on_execution_complete

    if let Some((fmt, results_file_path)) = output_settings {
        omni_file_data_serde::write_with_format_async(
            fmt.to_serde_format(),
            &results_file_path,
            &results,
            ctx.sys(),
        )
        .await?;
    }

    Ok(exit_code(&results))
}
