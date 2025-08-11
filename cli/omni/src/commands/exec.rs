use clap::Args;

use crate::{
    context::Context,
    executor::{Call, TaskOrchestrator},
};

#[derive(Args, Debug)]
pub struct ExecArgs {
    #[arg(required = true)]
    command: String,
    #[arg(num_args(0..))]
    args: Vec<String>,
    #[arg(
        long,
        short,
        help = "Run the command based on the project name matching the filter"
    )]
    filter: Option<String>,
}

#[derive(Args)]
pub struct ExecCommand {
    #[command(flatten)]
    args: ExecArgs,
}

pub async fn run(command: &ExecCommand, ctx: &Context) -> eyre::Result<()> {
    let mut builder = TaskOrchestrator::builder();

    builder
        .context(ctx.clone())
        .call(Call::new_command(command.args.command.clone()));

    if let Some(filter) = &command.args.filter {
        builder.project_filter(filter);
    }

    let orchestrator = builder.build()?;

    let results = orchestrator.execute().await?;

    trace::info!("Results: {:#?}", results);

    Ok(())
}
