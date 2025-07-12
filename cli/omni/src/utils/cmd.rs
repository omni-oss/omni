use std::{collections::HashMap, ffi::OsString, path::Path};

use deno_task_shell::KillSignal;

pub async fn run(
    command: &str,
    cwd: &Path,
    env_vars: &HashMap<OsString, OsString>,
) -> eyre::Result<i32> {
    let list = deno_task_shell::parser::parse(command).map_err(|_| {
        eyre::eyre!("Failed to parse task command: '{}'", command)
    })?;

    let exit_status = deno_task_shell::execute(
        list,
        env_vars.clone(),
        cwd.to_path_buf(),
        Default::default(),
        KillSignal::default(),
    )
    .await;

    Ok(exit_status)
}
