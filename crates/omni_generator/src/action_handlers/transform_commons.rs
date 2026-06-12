use std::{path::Path, process::Stdio};

use maps::Map;
use omni_generator_configurations::CommonRunCustomActionConfiguration;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    process::Command,
};

use crate::{
    GeneratorSys,
    action_handlers::{
        HandlerContext,
        run_custom_commons::{build_command_env, target_path},
    },
    error::{Error, ErrorInner},
};

/// Reads `file` through `sys`, pipes its current contents through `command`
/// (writing the contents to the command's standard input and exposing the
/// file's path via the `FILENAME` environment variable) and, when the command
/// exits successfully, replaces the file's contents with the command's standard
/// output.
///
/// This is the shared implementation behind both the `transform` and
/// `transform-many` actions.
pub(crate) async fn transform_one(
    file: &Path,
    command: &str,
    common: &CommonRunCustomActionConfiguration,
    ctx: &HandlerContext<'_>,
    sys: &impl GeneratorSys,
) -> Result<(), Error> {
    // Honour the same dry-run contract as the other "run" actions: skip the
    // (potentially side-effecting) command unless it explicitly opts in.
    if ctx.dry_run && !common.supports_dry_run {
        log::info!("Skipped transforming {} (dry run)", file.display());
        return Ok(());
    }

    let content = sys
        .fs_read_async(file)
        .await
        .map_err(|e| ErrorInner::new_failed_to_read_file(file, e))?;

    let cwd = target_path(common, ctx, ctx.gen_session, sys).await?;

    let command = omni_tera::one_off(
        command,
        format!("command for {}", ctx.resolved_action_name),
        ctx.tera_context_values,
    )?;

    let mut env = build_command_env(common, ctx, &cwd)?.into_owned();
    env.insert("FILENAME".to_string(), file.to_string_lossy().into_owned());

    // Allow `$FILENAME` (and any other env var) to be referenced directly in
    // the command string, mirroring `run-command`'s command expansion.
    let command = ::env::expand(&command, &env);

    log::info!(
        "Transforming {} through command: {}",
        file.display(),
        command
    );

    let output = run_transform_command(&command, &cwd, &env, &content).await?;

    if output.exit_code != 0 {
        return Err(ErrorInner::CommandFailed {
            command,
            exit_code: output.exit_code,
        })?;
    }

    sys.fs_write_async(file, &output.stdout)
        .await
        .map_err(|e| ErrorInner::new_failed_to_write_file(file, e))?;

    log::info!("Wrote transformed output to {}", file.display());

    Ok(())
}

/// The captured result of running a transform command.
struct TransformOutput {
    stdout: Vec<u8>,
    exit_code: u32,
}

/// Runs `command` with `cwd` as its working directory and `env` layered on top
/// of the inherited environment, feeding `input` to its standard input and
/// capturing its standard output.
///
/// A plain piped process (never a PTY) is used so that standard output is
/// captured verbatim and never interleaved with standard error.
async fn run_transform_command(
    command: &str,
    cwd: &Path,
    env: &Map<String, String>,
    input: &[u8],
) -> Result<TransformOutput, Error> {
    let parsed = shlex::split(command).ok_or_else(|| {
        Error::custom(format!("could not parse command: {command}"))
    })?;

    let (program, args) = parsed
        .split_first()
        .ok_or_else(|| Error::custom("cannot run an empty command"))?;

    // The child runs as a real process, so its working directory must exist on
    // the real file system. During a transaction the target/output directory is
    // often still buffered in the overlay and not yet on disk, so fall back to
    // the nearest existing ancestor. Tool config discovery is preserved because
    // the not-yet-created leaf directories cannot contain config anyway, and we
    // must not create them here (that would be an uncommitted side effect).
    let spawn_cwd = nearest_existing_dir(cwd);

    log::trace!(
        "running command: {program} {args:?}, cwd: {spawn_cwd:?}, env: {env:?}"
    );

    let mut cmd = Command::new(program);
    if !args.is_empty() {
        cmd.args(args);
    }

    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .envs(env);

    if let Some(spawn_cwd) = spawn_cwd {
        cmd.current_dir(spawn_cwd);
    }

    let mut child = cmd.spawn().map_err(|e| {
        Error::custom(format!(
            "failed to spawn command '{program}' (cwd: {}): {e}",
            spawn_cwd.unwrap_or(cwd).display()
        ))
    })?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| Error::custom("could not capture command stdin"))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::custom("could not capture command stdout"))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| Error::custom("could not capture command stderr"))?;

    // Feed the input on a separate task so that writing stdin and draining
    // stdout/stderr happen concurrently (avoiding a pipe-buffer deadlock).
    let input = input.to_vec();
    let write_task = tokio::spawn(async move {
        stdin.write_all(&input).await?;
        stdin.shutdown().await?;
        Ok::<_, std::io::Error>(())
    });

    let mut stdout_buf = Vec::new();
    let mut stderr_buf = Vec::new();

    let (stdout_res, stderr_res) = tokio::join!(
        stdout.read_to_end(&mut stdout_buf),
        stderr.read_to_end(&mut stderr_buf),
    );
    stdout_res?;
    stderr_res?;

    // A command may legitimately exit before consuming all of its input, which
    // surfaces as a broken pipe on our side; that is not an error.
    match write_task.await {
        Ok(Ok(())) => {}
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::BrokenPipe => {}
        Ok(Err(e)) => return Err(e.into()),
        Err(e) => {
            return Err(Error::custom(format!(
                "failed to write to command stdin: {e}"
            )));
        }
    }

    let status = child.wait().await?;
    let exit_code = status.code().unwrap_or(0) as u32;

    if !stderr_buf.is_empty() {
        log::debug!("command stderr: {}", String::from_utf8_lossy(&stderr_buf));
    }

    Ok(TransformOutput {
        stdout: stdout_buf,
        exit_code,
    })
}

/// Returns the deepest ancestor of `dir` (including `dir` itself) that exists
/// on the real file system as a directory, or `None` if there is no such
/// ancestor (in which case the caller should leave the child's working
/// directory inherited from the current process).
fn nearest_existing_dir(dir: &Path) -> Option<&Path> {
    dir.ancestors()
        .find(|ancestor| !ancestor.as_os_str().is_empty() && ancestor.is_dir())
}

#[cfg(test)]
mod tests {
    use maps::Map;

    use super::*;

    fn cwd() -> std::path::PathBuf {
        std::env::temp_dir()
    }

    /// A command that copies its standard input to its standard output.
    fn passthrough_cmd() -> &'static str {
        if cfg!(windows) {
            // `findstr` matches every (non-empty) line and echoes it back.
            "findstr /R .*"
        } else {
            "cat"
        }
    }

    /// A command that prints the `FILENAME` environment variable to standard
    /// output without reading its standard input.
    fn print_filename_cmd() -> &'static str {
        if cfg!(windows) {
            "cmd /C echo %FILENAME%"
        } else {
            "printenv FILENAME"
        }
    }

    /// A command that exits with a non-zero status.
    fn failing_cmd() -> &'static str {
        if cfg!(windows) {
            "cmd /C exit 1"
        } else {
            "false"
        }
    }

    /// Normalizes captured output for cross-platform comparison by dropping
    /// carriage returns (Windows line endings) and any trailing newline.
    fn normalized(bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes)
            .replace('\r', "")
            .trim_end_matches('\n')
            .to_string()
    }

    #[tokio::test]
    async fn pipes_stdin_through_to_stdout() {
        let out = run_transform_command(
            passthrough_cmd(),
            &cwd(),
            &Map::default(),
            b"hello world\n",
        )
        .await
        .unwrap();

        assert_eq!(out.exit_code, 0);
        assert_eq!(normalized(&out.stdout), "hello world");
    }

    #[tokio::test]
    async fn exposes_env_and_ignores_unread_stdin() {
        let mut env = Map::default();
        env.insert("FILENAME".to_string(), "example.ts".to_string());

        // The command never reads stdin and exits immediately; the resulting
        // broken pipe on our writer must be tolerated.
        let out = run_transform_command(
            print_filename_cmd(),
            &cwd(),
            &env,
            b"ignored",
        )
        .await
        .unwrap();

        assert_eq!(out.exit_code, 0);
        assert_eq!(normalized(&out.stdout), "example.ts");
    }

    #[tokio::test]
    async fn reports_nonzero_exit_code() {
        let out =
            run_transform_command(failing_cmd(), &cwd(), &Map::default(), b"")
                .await
                .unwrap();

        assert_ne!(out.exit_code, 0);
    }

    /// Byte-exact guarantee that standard output is captured verbatim and never
    /// interleaved with standard error. Relies on POSIX utilities.
    #[cfg(unix)]
    #[tokio::test]
    async fn captures_stdout_verbatim_without_stderr() {
        // Writes to both streams; only stdout must be captured, verbatim.
        let out = run_transform_command(
            "sh -c 'cat; echo noise 1>&2'",
            &cwd(),
            &Map::default(),
            b"exact-bytes",
        )
        .await
        .unwrap();

        assert_eq!(out.exit_code, 0);
        assert_eq!(out.stdout, b"exact-bytes");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn transforms_input_through_command() {
        let out = run_transform_command(
            "tr a-z A-Z",
            &cwd(),
            &Map::default(),
            b"abc\n",
        )
        .await
        .unwrap();

        assert_eq!(out.exit_code, 0);
        assert_eq!(out.stdout, b"ABC\n");
    }
}
