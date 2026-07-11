use std::borrow::Cow;

use maps::Map;

use crate::CommandConfig;

/// Errors produced while resolving a [`CommandConfig`] into a concrete argv.
#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("invalid shell command syntax")]
    InvalidShellSyntax,

    #[error(transparent)]
    Tera(#[from] omni_tera::Error),

    #[error("command resolved to an empty argv")]
    EmptyArgv,
}

/// A [`CommandConfig`] resolved into a program and its arguments.
///
/// `prog` is `None` when the command resolved to an empty argv (e.g. an empty
/// shell string or an empty argv list). In that case `args` is always empty
/// too. This lets callers decide whether an empty command is an error or a
/// no-op without having to index into a possibly-empty vector themselves.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Command {
    pub prog: Option<String>,
    pub args: Vec<String>,
}

impl Command {
    /// Build a [`ResolvedCommand`] from an already-expanded argv, splitting the
    /// first element off as the program.
    ///
    /// Performs the bounds checking so callers never index an empty argv: an
    /// empty `argv` yields `prog: None` and no args.
    fn from_argv(mut argv: Vec<String>) -> Self {
        if argv.is_empty() {
            return Self::default();
        }

        let prog = argv.remove(0);
        Self {
            prog: Some(prog),
            args: argv,
        }
    }

    /// Whether the command resolved to no program at all.
    pub fn is_empty(&self) -> bool {
        self.prog.is_none()
    }

    /// The full argv (`prog` followed by `args`), empty when there is no
    /// program.
    pub fn to_argv(&self) -> Vec<String> {
        match &self.prog {
            Some(prog) => {
                let mut argv = Vec::with_capacity(self.args.len() + 1);
                argv.push(prog.clone());
                argv.extend(self.args.iter().cloned());
                argv
            }
            None => Vec::new(),
        }
    }

    pub fn as_ref(&self) -> CommandRef<'_> {
        CommandRef {
            prog: self.prog.as_deref(),
            args: self.args.as_slice(),
        }
    }
}

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CommandRef<'a> {
    pub prog: Option<&'a str>,
    pub args: &'a [String],
}

/// Whether a string contains Tera template markers (`{{` or `{%`).
///
/// Used to skip the (relatively expensive) `omni_tera::one_off` render for
/// literal strings.
#[inline]
fn has_template_markers(s: &str) -> bool {
    s.contains("{{") || s.contains("{%") || s.contains("{#")
}

/// Render a string through Tera only when a context is provided *and* the
/// string actually contains template markers; otherwise borrow it unchanged.
fn maybe_render<'a>(
    s: &'a str,
    tera_ctx: Option<&omni_tera::Context>,
) -> Result<Cow<'a, str>, ResolveError> {
    match tera_ctx {
        Some(ctx) if has_template_markers(s) => {
            Ok(Cow::Owned(omni_tera::one_off(s, "command", ctx)?))
        }
        _ => Ok(Cow::Borrowed(s)),
    }
}

/// Resolve a [`CommandConfig`] into a concrete [`Command`].
///
/// Both `tera_ctx` and `env_vars` are optional; callers that lack one or both
/// simply pass `None` and the corresponding expansion step is skipped.
///
/// For [`CommandConfig::Shell`] the order is **Tera -> env-expand -> shlex
/// split**, matching the historical behavior (only the split now uses `shlex`
/// uniformly on every platform). For [`CommandConfig::Argv`] each element is
/// Tera- and env-expanded independently and never split.
///
/// In both cases the first resolved element becomes [`ResolvedCommand::prog`]
/// and the rest become [`ResolvedCommand::args`]. An empty result yields a
/// [`Command`] with `prog: None` rather than panicking.
pub fn resolve_command(
    spec: &CommandConfig,
    tera_ctx: Option<&omni_tera::Context>,
    env_vars: Option<&Map<String, String>>,
) -> Result<Command, ResolveError> {
    match spec {
        CommandConfig::Shell(s) => {
            let s = maybe_render(s, tera_ctx)?;
            let s = match env_vars {
                Some(vars) => Cow::Owned(::env::expand(s.as_ref(), vars)),
                None => s,
            };
            let argv = shlex::split(s.as_ref())
                .ok_or(ResolveError::InvalidShellSyntax)?;

            Ok(Command::from_argv(argv))
        }
        CommandConfig::Argv(items) => {
            let argv = items
                .iter()
                .map(|e| {
                    let e = maybe_render(e, tera_ctx)?;
                    Ok(match env_vars {
                        Some(vars) => ::env::expand(e.as_ref(), vars),
                        None => e.into_owned(),
                    })
                })
                .collect::<Result<Vec<String>, ResolveError>>()?;

            Ok(Command::from_argv(argv))
        }
    }
}

#[cfg(test)]
mod tests {
    use maps::Map;
    use omni_tera::Context;

    use super::*;

    fn env(pairs: &[(&str, &str)]) -> Map<String, String> {
        let mut m = Map::default();
        for (k, v) in pairs {
            m.insert((*k).to_string(), (*v).to_string());
        }
        m
    }

    fn shell(s: &str) -> CommandConfig {
        CommandConfig::Shell(s.to_string())
    }

    fn argv(items: &[&str]) -> CommandConfig {
        CommandConfig::Argv(items.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn shell_splits_prog_and_args() {
        let resolved =
            resolve_command(&shell("echo hello world"), None, None).unwrap();
        assert_eq!(resolved.prog.as_deref(), Some("echo"));
        assert_eq!(resolved.args, vec!["hello", "world"]);
        assert!(!resolved.is_empty());
    }

    #[test]
    fn shell_single_token_has_no_args() {
        let resolved = resolve_command(&shell("ls"), None, None).unwrap();
        assert_eq!(resolved.prog.as_deref(), Some("ls"));
        assert!(resolved.args.is_empty());
    }

    #[test]
    fn shell_empty_string_yields_no_prog() {
        let resolved = resolve_command(&shell(""), None, None).unwrap();
        assert!(resolved.is_empty());
        assert_eq!(resolved.prog, None);
        assert!(resolved.args.is_empty());
    }

    #[test]
    fn shell_whitespace_only_yields_no_prog() {
        let resolved = resolve_command(&shell("   \t  "), None, None).unwrap();
        assert!(resolved.is_empty());
        assert_eq!(resolved.prog, None);
        assert!(resolved.args.is_empty());
    }

    #[test]
    fn shell_single_quotes_stay_one_arg() {
        let resolved = resolve_command(
            &shell("bun exec 'cp -r ../a/dist ./dist'"),
            None,
            None,
        )
        .unwrap();
        assert_eq!(resolved.prog.as_deref(), Some("bun"));
        assert_eq!(resolved.args, vec!["exec", "cp -r ../a/dist ./dist"]);
    }

    #[test]
    fn shell_invalid_syntax_errors() {
        // Unbalanced quote cannot be split by shlex.
        let err = resolve_command(&shell("echo 'unterminated"), None, None)
            .unwrap_err();
        assert!(matches!(err, ResolveError::InvalidShellSyntax));
    }

    #[test]
    fn shell_env_expansion() {
        let resolved = resolve_command(
            &shell("echo $GREETING"),
            None,
            Some(&env(&[("GREETING", "hello")])),
        )
        .unwrap();
        assert_eq!(resolved.prog.as_deref(), Some("echo"));
        assert_eq!(resolved.args, vec!["hello"]);
    }

    #[test]
    fn shell_tera_expansion() {
        let mut ctx = Context::new();
        ctx.insert("name", "world");
        let resolved =
            resolve_command(&shell("echo {{ name }}"), Some(&ctx), None)
                .unwrap();
        assert_eq!(resolved.prog.as_deref(), Some("echo"));
        assert_eq!(resolved.args, vec!["world"]);
    }

    #[test]
    fn shell_remove_tera_comments() {
        let resolved = resolve_command(
            &shell("echo {# comment #}"),
            Some(&Context::new()),
            None,
        )
        .unwrap();
        assert_eq!(resolved.prog.as_deref(), Some("echo"));
        assert!(resolved.args.is_empty());
    }

    #[test]
    fn shell_none_contexts_skip_steps() {
        let resolved =
            resolve_command(&shell("echo $NOPE {{ nope }}"), None, None)
                .unwrap();
        // No env-expand and no Tera render: literal passthrough (shlex split).
        assert_eq!(
            resolved.to_argv(),
            vec!["echo", "$NOPE", "{{", "nope", "}}"]
        );
    }

    #[test]
    fn argv_element_with_spaces_stays_one_arg() {
        let resolved = resolve_command(
            &argv(&["bun", "exec", "cp -r ../a/dist ./dist"]),
            None,
            None,
        )
        .unwrap();
        assert_eq!(resolved.prog.as_deref(), Some("bun"));
        assert_eq!(resolved.args, vec!["exec", "cp -r ../a/dist ./dist"]);
    }

    #[test]
    fn argv_empty_list_yields_no_prog() {
        let resolved = resolve_command(&argv(&[]), None, None).unwrap();
        assert!(resolved.is_empty());
        assert_eq!(resolved.prog, None);
        assert!(resolved.args.is_empty());
    }

    #[test]
    fn argv_single_element_has_no_args() {
        let resolved = resolve_command(&argv(&["ls"]), None, None).unwrap();
        assert_eq!(resolved.prog.as_deref(), Some("ls"));
        assert!(resolved.args.is_empty());
    }

    #[test]
    fn argv_env_expansion_per_element_no_split() {
        let resolved = resolve_command(
            &argv(&["echo", "$GREETING world"]),
            None,
            Some(&env(&[("GREETING", "hello")])),
        )
        .unwrap();
        // Env-expanded, but not split: still one argument.
        assert_eq!(resolved.prog.as_deref(), Some("echo"));
        assert_eq!(resolved.args, vec!["hello world"]);
    }

    #[test]
    fn argv_tera_expansion_no_split() {
        let mut ctx = Context::new();
        ctx.insert("msg", "a b c");
        let resolved =
            resolve_command(&argv(&["echo", "{{ msg }}"]), Some(&ctx), None)
                .unwrap();
        assert_eq!(resolved.prog.as_deref(), Some("echo"));
        assert_eq!(resolved.args, vec!["a b c"]);
    }

    #[test]
    fn argv_remove_tera_comments() {
        let resolved = resolve_command(
            &argv(&["echo", "{# comment #}"]),
            Some(&Context::new()),
            None,
        )
        .unwrap();
        assert_eq!(resolved.prog.as_deref(), Some("echo"));
        assert!(resolved.args.len() == 1);
        assert!(resolved.args[0].is_empty());
    }

    #[test]
    fn to_argv_roundtrips() {
        let resolved =
            resolve_command(&argv(&["echo", "a", "b"]), None, None).unwrap();
        assert_eq!(resolved.to_argv(), vec!["echo", "a", "b"]);
    }

    #[test]
    fn to_argv_of_empty_is_empty() {
        let resolved = Command::default();
        assert!(resolved.to_argv().is_empty());
    }
}
