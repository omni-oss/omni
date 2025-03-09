use std::process::Stdio;

use clap::Args;
use tokio::process;

use crate::context::Context;

#[derive(Args, Debug)]
pub struct ExecArgs {
    #[arg(required = true)]
    command: String,
    #[arg(num_args(0..))]
    args: Vec<String>,
}

#[derive(Args)]
pub struct ExecCommand {
    #[command(flatten)]
    args: ExecArgs,
}

pub async fn run(command: &ExecCommand, ctx: &Context) -> eyre::Result<()> {
    let mut cmd = process::Command::new(&command.args.command);

    tracing::info!(
        "executing command: {} {:?}",
        command.args.command,
        command.args.args
    );

    let exit = cmd
        .args(&command.args.args)
        .envs(ctx.get_all_env())
        .stderr(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stdin(Stdio::inherit())
        .spawn()?
        .wait()
        .await?;

    if !exit.success() {
        eyre::bail!("command exited with non-zero status: {}", exit);
    }

    Ok(())
}
