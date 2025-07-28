use std::{collections::HashMap, sync::Arc};

use clap::Args;
use futures::future::join_all;
use omni_core::TaskExecutionNode;
use tokio::sync::Mutex;

use crate::{
    commands::utils::execute_task, context::Context,
    utils::dir_walker::create_default_dir_walker,
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

pub async fn run(command: &ExecCommand, ctx: &mut Context) -> eyre::Result<()> {
    ctx.load_projects(&create_default_dir_walker())?;
    let filter = if let Some(filter) = &command.args.filter {
        filter
    } else {
        "*"
    };
    let projects = ctx
        .get_filtered_projects(filter)?
        .iter()
        .map(|a| (*a).clone())
        .collect::<Vec<_>>();

    if projects.is_empty() {
        eyre::bail!("No project found for filter: {}", filter);
    }

    trace::debug!(
        "executing command: {} {:?}",
        command.args.command,
        command.args.args
    );
    trace::debug!("Projects: {:?}", projects);
    let mut futures = vec![];

    let full_cmd =
        format!("{} {}", command.args.command, command.args.args.join(" "));

    let dir_envs = Arc::new(Mutex::new(HashMap::new()));
    let shared_ctx = Arc::new(Mutex::new(ctx.clone()));
    for p in projects {
        let full_cmd = full_cmd.clone();
        futures.push(execute_task(
            TaskExecutionNode::new(
                "exec".to_string(),
                full_cmd.clone(),
                p.name.clone(),
                p.dir.clone(),
            ),
            shared_ctx.clone(),
            dir_envs.clone(),
        ));
    }

    let results = join_all(futures).await;

    let failed_count = results.iter().filter(|r| r.is_err()).count();

    if failed_count > 0 {
        trace::error!(
            "Failed to execute command '{}' in {} projects",
            &full_cmd,
            failed_count
        );
    }

    Ok(())
}
