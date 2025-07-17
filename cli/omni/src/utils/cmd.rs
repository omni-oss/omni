use std::{collections::HashMap, ffi::OsString, path::Path};

use deno_task_shell::{
    KillSignal, ShellPipeReader, ShellPipeWriter, ShellState,
};

pub async fn run(
    command: &str,
    cwd: &Path,
    env_vars: HashMap<OsString, OsString>,
) -> eyre::Result<i32> {
    run_with_pipes(
        command,
        cwd,
        env_vars,
        ShellPipeReader::stdin(),
        ShellPipeWriter::stdout(),
        ShellPipeWriter::stderr(),
    )
    .await
}

pub async fn run_with_pipes(
    command: &str,
    cwd: &Path,
    env_vars: HashMap<OsString, OsString>,
    stdin: ShellPipeReader,
    stdout: ShellPipeWriter,
    stderr: ShellPipeWriter,
) -> eyre::Result<i32> {
    let list = deno_task_shell::parser::parse(command).map_err(|_| {
        eyre::eyre!("Failed to parse task command: '{}'", command)
    })?;

    let shell_state = ShellState::new(
        env_vars,
        cwd.to_path_buf(),
        Default::default(),
        KillSignal::default(),
    );

    let exit_status = deno_task_shell::execute_with_pipes(
        list,
        shell_state,
        stdin,
        stdout,
        stderr,
    )
    .await;

    Ok(exit_status)
}
