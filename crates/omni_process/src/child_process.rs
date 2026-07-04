use std::{
    borrow::Cow, collections::HashMap, ffi::OsString, path::PathBuf, pin::Pin,
};

use bytes::{BufMut as _, Bytes, BytesMut};
use derive_new::new;
use futures::future::try_join_all;
use maps::Map;
use std::path::Path;
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use system_traits::auto_impl;
use tokio::io::{
    AsyncBufReadExt as _, AsyncRead, AsyncReadExt as _, AsyncWrite,
    AsyncWriteExt as _, BufReader,
};
use trace::Level;

use crate::{Child, ChildError};

#[auto_impl]
pub trait ChildProcessWriter: AsyncWrite + Send {}

#[auto_impl]
pub trait ChildProcessReader: AsyncRead + Send {}

pub trait CommandProvider<'a>: 'a {
    fn command(&'a self) -> Cow<'a, str>;
}

pub trait CurrentDirProvider<'a>: 'a {
    fn current_dir(&'a self) -> Cow<'a, Path>;
}

impl<'a> CommandProvider<'a> for String {
    fn command(&'a self) -> Cow<'a, str> {
        self.into()
    }
}
impl<'a> CurrentDirProvider<'a> for PathBuf {
    fn current_dir(&'a self) -> Cow<'a, Path> {
        self.into()
    }
}

#[derive(new)]
pub struct ChildProcess<
    C: for<'a> CommandProvider<'a>,
    D: for<'a> CurrentDirProvider<'a>,
> {
    #[new(into)]
    command: C,

    #[new(into)]
    current_dir: D,

    #[new(default)]
    expanded_command: Option<String>,

    #[new(default)]
    output_writer: Option<Pin<Box<dyn ChildProcessWriter>>>,

    #[new(default)]
    input_reader: Option<Pin<Box<dyn ChildProcessReader>>>,

    #[new(default)]
    env_vars: Option<HashMap<OsString, OsString>>,

    #[new(default)]
    record_logs: bool,

    #[new(default)]
    keep_stdin_open: bool,

    #[new(default)]
    empty_command_is_success: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, new)]
pub struct ChildProcessResult {
    #[new(into)]
    pub exit_code: u32,
    #[new(into)]
    pub elapsed: std::time::Duration,
    #[new(into)]
    pub logs: Option<Bytes>,
}

impl ChildProcessResult {
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }

    pub fn exit_code(&self) -> u32 {
        self.exit_code
    }
}

impl<C: for<'a> CommandProvider<'a>, D: for<'a> CurrentDirProvider<'a>>
    ChildProcess<C, D>
{
    pub fn output_writer(
        &mut self,
        writer: impl ChildProcessWriter + 'static,
    ) -> &mut Self {
        self.output_writer = Some(Box::pin(writer));
        self
    }

    pub fn env_vars(&mut self, vars: &Map<String, String>) -> &mut Self {
        self.expanded_command =
            Some(::env::expand(self.command.command().as_ref(), vars));
        self.env_vars = Some(vars_os(vars));

        self
    }

    pub fn keep_stdin_open(&mut self, keep_stdin_open: bool) -> &mut Self {
        self.keep_stdin_open = keep_stdin_open;
        self
    }

    pub fn input_reader(
        &mut self,
        reader: impl ChildProcessReader + 'static,
    ) -> &mut Self {
        self.input_reader = Some(Box::pin(reader));

        self
    }

    pub fn record_logs(&mut self, record_logs: bool) -> &mut Self {
        self.record_logs = record_logs;
        self
    }

    pub fn empty_command_is_success(
        &mut self,
        empty_command_is_success: bool,
    ) -> &mut Self {
        self.empty_command_is_success = empty_command_is_success;
        self
    }

    #[cfg_attr(
        feature = "enable-tracing",
        tracing::instrument(level = Level::DEBUG, skip_all)
    )]
    pub async fn exec(
        mut self,
    ) -> Result<ChildProcessResult, ChildProcessError> {
        let unexpanded_comand = self.command.command();

        if unexpanded_comand.trim().is_empty() {
            if self.empty_command_is_success {
                return Ok(ChildProcessResult {
                    exit_code: 0,
                    elapsed: std::time::Duration::default(),
                    logs: None,
                });
            } else {
                return Err(ChildProcessError::no_command());
            }
        }

        let start_time = std::time::Instant::now();

        let command = if let Some(command) = self.expanded_command.as_ref() {
            command.as_str()
        } else {
            unexpanded_comand.as_ref()
        };

        let parsed = split_command(command).ok_or_else(|| {
            ChildProcessErrorInner::CantParseCommand(command.to_string())
        })?;

        log::trace!("executing command: {:?}", parsed);

        let child = Child::spawn(
            parsed[0].clone(),
            parsed.iter().skip(1).cloned().collect::<Vec<_>>(),
            self.current_dir.current_dir().as_ref(),
            self.env_vars.unwrap_or_default(),
        )?;

        let stdout = child
            .take_output_reader()
            .ok_or(ChildProcessErrorInner::CantTakeStdout)?;

        let stderr = child
            .take_error_reader()
            .ok_or(ChildProcessErrorInner::CantTakeStderr)?;

        let mut input = child
            .take_input_writer()
            .ok_or(ChildProcessErrorInner::CantTakeStdin)?;

        let mut tasks = vec![];

        let mut writer = self.output_writer.take();
        let logs_output_task = tokio::spawn(async move {
            if !self.record_logs && writer.is_none() {
                log::trace!("no logs output, exit early");
                return Ok::<_, ChildProcessError>(None);
            }

            let mut logs_output = if self.record_logs {
                Some(BytesMut::new())
            } else {
                None
            };

            log::trace!("logs output task started");

            let mut stderr = stderr.map(BufReader::new);
            let mut stdout = BufReader::new(stdout);
            loop {
                let n;
                let line;
                if let Some(stderr_mut) = stderr.as_mut() {
                    let mut stdout_line = String::new();
                    let mut stderr_line = String::new();
                    tokio::select! {
                        res = stderr_mut.read_line(&mut stderr_line) => {
                            n = res?;
                            if n == 0 {
                                stderr = None;
                                log::trace!("stderr is empty, breaking");
                                continue;
                            }
                            line = stderr_line;
                        }
                        res = stdout.read_line(&mut stdout_line) => {
                            n = res?;
                            if n == 0 {
                                log::trace!("stdout is empty, breaking");
                                break;
                            }
                            line = stdout_line;
                        }
                    }
                } else {
                    let mut stdout_line = String::new();
                    n = stdout.read_line(&mut stdout_line).await?;
                    if n == 0 {
                        log::trace!("stdout is empty, breaking");
                        break;
                    }
                    line = stdout_line;
                }

                log::trace!("received log chunk to write: {}", n);

                if let Some(logs_output) = &mut logs_output {
                    log::trace!("writing log chunk to logs output");
                    logs_output.put_slice(line.as_bytes());
                }

                if let Some(writer) = writer.as_mut() {
                    log::trace!("writing log chunk to output writer");
                    writer.write_all(line.as_bytes()).await?;
                }
            }
            log::trace!("logs output task done");
            Ok::<_, ChildProcessError>(logs_output.map(|b| b.freeze()))
        });

        if let Some(input_reader) = self.input_reader {
            let stdin_task = {
                tokio::spawn(async move {
                    tokio::io::copy(
                        &mut input_reader.take(u64::MAX),
                        &mut input,
                    )
                    .await?;

                    Ok::<_, ChildProcessError>(())
                })
            };

            tasks.push(stdin_task);
        } else if !self.keep_stdin_open {
            trace::trace!("dropping_input");
            std::mem::drop(input);
        }

        let all_tasks = try_join_all(tasks);

        let (logs_output, vec_result, exit_status) =
            tokio::join!(logs_output_task, all_tasks, child.wait());

        let _ = vec_result?;
        let logs = logs_output??;

        let exit_code = exit_status?;

        let elapsed = start_time.elapsed();

        Ok(ChildProcessResult {
            exit_code,
            elapsed,
            logs,
        })
    }
}

fn vars_os(vars: &Map<String, String>) -> HashMap<OsString, OsString> {
    vars.iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect::<HashMap<_, _>>()
}

/// Split a command string into program + args.
///
/// The two platforms follow different command-line conventions, so we parse
/// with the matching rules instead of forcing one lexer everywhere:
///
/// * Unix uses [`shlex`] (POSIX shell word-splitting), where `\` escapes the
///   next character.
/// * Windows uses [`split_command_windows`], which follows the
///   `CommandLineToArgvW`/MSVCRT rules the OS itself uses. There `\` is an
///   ordinary character (so paths like `C:\Users` survive) and is only
///   special immediately before a `"`.
fn split_command(command: &str) -> Option<Vec<String>> {
    #[cfg(windows)]
    {
        Some(split_command_windows(command))
    }
    #[cfg(not(windows))]
    {
        shlex::split(command)
    }
}

/// Split a Windows command line into arguments using the same algorithm as
/// `CommandLineToArgvW` / the MSVCRT runtime.
///
/// Rules:
/// * Arguments are separated by unquoted whitespace (space or tab).
/// * A `"` toggles "in quotes" mode; inside quotes, whitespace is literal.
/// * A run of backslashes is only special when immediately followed by a `"`:
///   - `2n` backslashes + `"` => `n` backslashes, and the `"` is a delimiter.
///   - `2n+1` backslashes + `"` => `n` backslashes followed by a literal `"`.
///   - Backslashes not followed by `"` are all literal.
/// * Inside quotes, a doubled `""` emits a single literal `"` and stays quoted.
///
/// Unlike the OS, we apply these rules uniformly to every argument (including
/// the program name); the program-name special-case only matters for embedded
/// backslashes, which we want kept literally anyway.
#[cfg_attr(not(windows), allow(dead_code))]
fn split_command_windows(command: &str) -> Vec<String> {
    let chars: Vec<char> = command.chars().collect();
    let mut args: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut in_arg = false;
    let mut in_quotes = false;
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            '\\' => {
                // Count the run of backslashes.
                let mut backslashes = 0;
                while i < chars.len() && chars[i] == '\\' {
                    backslashes += 1;
                    i += 1;
                }
                in_arg = true;
                if i < chars.len() && chars[i] == '"' {
                    // Backslashes escape each other and possibly the quote.
                    for _ in 0..(backslashes / 2) {
                        cur.push('\\');
                    }
                    if backslashes % 2 == 1 {
                        // Odd: the quote is escaped -> literal, consume it.
                        cur.push('"');
                        i += 1;
                    }
                    // Even: leave the quote for the next iteration to toggle.
                } else {
                    // Not before a quote: all backslashes are literal.
                    for _ in 0..backslashes {
                        cur.push('\\');
                    }
                }
            }
            '"' => {
                in_arg = true;
                if in_quotes && i + 1 < chars.len() && chars[i + 1] == '"' {
                    // "" inside quotes -> a single literal quote.
                    cur.push('"');
                    i += 2;
                } else {
                    in_quotes = !in_quotes;
                    i += 1;
                }
            }
            c if (c == ' ' || c == '\t') && !in_quotes => {
                if in_arg {
                    args.push(std::mem::take(&mut cur));
                    in_arg = false;
                }
                i += 1;
            }
            c => {
                cur.push(c);
                in_arg = true;
                i += 1;
            }
        }
    }

    if in_arg {
        args.push(cur);
    }

    args
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct ChildProcessError(pub(crate) ChildProcessErrorInner);

impl ChildProcessError {
    pub fn custom<T: Into<eyre::Report>>(inner: T) -> Self {
        Self(ChildProcessErrorInner::Custom(inner.into()))
    }

    pub fn no_command() -> Self {
        Self(ChildProcessErrorInner::NoCommandProvided)
    }
}

impl<T: Into<ChildProcessErrorInner>> From<T> for ChildProcessError {
    fn from(value: T) -> Self {
        let inner = value.into();
        Self(inner)
    }
}

impl ChildProcessError {
    #[allow(unused)]
    pub fn kind(&self) -> ChildProcessErrorKind {
        self.0.discriminant()
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(name(ChildProcessErrorKind), vis(pub), repr(u8))]
#[allow(clippy::enum_variant_names)]
pub(crate) enum ChildProcessErrorInner {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("can't run command: {0}")]
    CantRunCommand(#[from] ChildError),

    #[error("can't parse command: {0}")]
    CantParseCommand(String),

    // #[error("can't get env vars")]
    // CantGetEnvVars,
    #[error("cant't take stdin")]
    CantTakeStdin,

    #[error("cant't take stdout")]
    CantTakeStdout,

    #[error("cant't take stderr")]
    CantTakeStderr,

    // #[error("cant't take stderr")]
    // CantTakeStderr,
    #[error(transparent)]
    Custom(#[from] eyre::Report),

    #[error(transparent)]
    Join(#[from] tokio::task::JoinError),

    #[error(transparent)]
    Mpsc(#[from] tokio::sync::mpsc::error::SendError<Bytes>),

    #[error("no command is provided")]
    NoCommandProvided,
}

#[cfg(test)]
mod tests {
    use super::split_command_windows as split;

    fn v(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn simple_words() {
        assert_eq!(split("vite dev"), v(&["vite", "dev"]));
    }

    #[test]
    fn collapses_and_trims_whitespace() {
        assert_eq!(split("  a\t b   c "), v(&["a", "b", "c"]));
        assert_eq!(split(""), Vec::<String>::new());
        assert_eq!(split("   "), Vec::<String>::new());
    }

    #[test]
    fn backslashes_are_literal_in_paths() {
        // The whole point: Windows paths must survive intact.
        assert_eq!(
            split(r"app --path C:\Users\P12C423\bin"),
            v(&["app", "--path", r"C:\Users\P12C423\bin"]),
        );
        assert_eq!(split(r"C:\tools\vite.exe"), v(&[r"C:\tools\vite.exe"]));
    }

    #[test]
    fn unc_path() {
        assert_eq!(
            split(r"copy \\server\share\file"),
            v(&["copy", r"\\server\share\file"]),
        );
    }

    #[test]
    fn quoted_arg_with_spaces() {
        assert_eq!(
            split(r#"app "C:\Program Files\app.exe" --flag"#),
            v(&["app", r"C:\Program Files\app.exe", "--flag"]),
        );
    }

    #[test]
    fn trailing_backslashes_before_closing_quote() {
        // 2n backslashes + " => n backslashes and the quote is a delimiter.
        assert_eq!(split(r#""C:\dir\\" x"#), v(&[r"C:\dir\", "x"]));
    }

    #[test]
    fn escaped_quote_is_literal() {
        // 2n+1 backslashes + " => n backslashes + a literal quote.
        assert_eq!(split(r#"echo \"hi\""#), v(&["echo", r#""hi""#]));
    }

    #[test]
    fn doubled_quote_inside_quotes_is_literal() {
        assert_eq!(split(r#""a""b""#), v(&[r#"a"b"#]));
    }

    #[test]
    fn empty_quoted_string_is_an_arg() {
        assert_eq!(split(r#"app "" x"#), v(&["app", "", "x"]));
    }
}
