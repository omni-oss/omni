use clap::Args;

use crate::{
    commands::utils::report_execution_results,
    context::Context,
    executor::{Call, TaskOrchestrator},
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
}

#[derive(Args)]
pub struct ExecCommand {
    #[command(flatten)]
    args: ExecArgs,
}

pub async fn run(command: &ExecCommand, ctx: &Context) -> eyre::Result<()> {
    let mut builder = TaskOrchestrator::builder();

    builder.context(ctx.clone()).call(Call::new_command(
        command.args.command.clone(),
        command.args.args.clone(),
    ));

    if let Some(filter) = &command.args.project {
        builder.project_filter(filter);
    }

    let orchestrator = builder.build()?;

    let results = orchestrator.execute().await?;

    report_execution_results(&results);

    Ok(())
}
