//! Cross-platform task launcher scripts.
//!
//! omni executes a task command by splitting it into `program + args` and
//! spawning the program **directly** (no shell) — on Windows the default piped
//! path is `Command::new(program)`, so a bare `echo`/redirection cannot create
//! a file. To make generated tasks produce their declared `dist/**` outputs
//! (so cold/forced benchmark runs exercise output collection + hashing), each
//! project ships a tiny launcher script — mirroring how `scripts/task-bench`
//! commits a `task.mjs` per project — invoked through the OS shell.
//!
//! The script is generated for the **host** OS (generation and execution
//! happen on the same machine), and its contents are fully deterministic for a
//! given project name + output count, preserving run-to-run reproducibility.

/// A generated launcher script: the file to write into the project directory
/// and its contents.
#[derive(Debug, Clone)]
pub struct Launcher {
    /// File name, relative to the project directory (e.g. `run.sh`).
    pub script_name: &'static str,
    /// Full script body.
    pub script_body: String,
}

/// Build the launcher script for a project. Each task invokes it with the task
/// name as `$1`/`%1`; the script writes `output_files` deterministic files to
/// `dist/<task>.<i>.txt`.
pub fn project_launcher(project: &str, output_files: usize) -> Launcher {
    #[cfg(windows)]
    {
        windows_launcher(project, output_files)
    }
    #[cfg(not(windows))]
    {
        unix_launcher(project, output_files)
    }
}

/// The omni task-command template for the host OS, with the `{task_id}`
/// placeholder [`omni_workspace_gen::render_omni`] expands to each task name.
/// Mirrors the launcher script name produced by [`project_launcher`].
pub fn task_command_template() -> String {
    #[cfg(windows)]
    {
        "cmd /C run.cmd {task_id}".to_string()
    }
    #[cfg(not(windows))]
    {
        "sh run.sh {task_id}".to_string()
    }
}

#[cfg_attr(windows, allow(dead_code))]
fn unix_launcher(project: &str, output_files: usize) -> Launcher {
    let mut body = String::from("#!/bin/sh\nmkdir -p dist\n");
    if output_files > 0 {
        body.push_str(&format!(
            "i=0\nwhile [ \"$i\" -lt {output_files} ]; do\n  \
             printf '%s' \"{project}:$1\" > \"dist/$1.$i.txt\"\n  \
             i=$((i + 1))\ndone\n",
        ));
    }
    Launcher {
        script_name: "run.sh",
        script_body: body,
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn windows_launcher(project: &str, output_files: usize) -> Launcher {
    let mut body =
        String::from("@echo off\r\nif not exist dist mkdir dist\r\n");
    if output_files > 0 {
        // Redirection-first form avoids `echo N>file` being parsed as a handle
        // redirect; `%%i` is the loop var, `%1` the task name.
        body.push_str(&format!(
            "for /L %%i in (0,1,{max}) do >dist\\%1.%%i.txt echo {project}:%1\r\n",
            max = output_files - 1,
        ));
    }
    Launcher {
        script_name: "run.cmd",
        script_body: body,
    }
}
