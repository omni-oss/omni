use std::borrow::Cow;

use maps::Map;

use crate::{CommandConfig, CommandOperand};

/// Errors produced while resolving a [`CommandConfig`] into a concrete argv.
#[derive(Debug, thiserror::Error, omni_diagnostic::Diagnostic)]
pub enum ResolveError {
    /// A configured shell command has invalid quoting or syntax.
    #[error("invalid shell command syntax")]
    #[diagnostic(
        code = "command.invalid_shell_syntax",
        hint = "check quoting and escaping in the configured command"
    )]
    InvalidShellSyntax,

    #[error(transparent)]
    #[diagnostic(skip)]
    Tera(#[from] omni_tera::Error),

    /// A configured command resolves to no executable.
    #[error("command resolved to an empty argv")]
    #[diagnostic(
        code = "command.empty_argv",
        hint = "provide a nonempty command with an executable name after expansion"
    )]
    EmptyArgv,
}

omni_codes::prefixes!("command");

#[cfg(test)]
mod diagnostic_tests {
    use eyre::WrapErr;
    use omni_diagnostic::{Audience, find};

    #[test]
    fn resolution_failures_are_registered_and_survive_context() {
        for (error, code) in [
            (
                super::ResolveError::InvalidShellSyntax,
                "command.invalid_shell_syntax",
            ),
            (super::ResolveError::EmptyArgv, "command.empty_argv"),
        ] {
            let report = Err::<(), _>(error)
                .wrap_err("resolving command")
                .unwrap_err();
            let diagnostic = find(&report).expect("command diagnostic");
            assert_eq!(diagnostic.code(), code);
            assert_eq!(diagnostic.audience(), Audience::User);
            assert_eq!(diagnostic.details(), None);
            assert!(!diagnostic.hints().is_empty());
            assert!(omni_codes::get(code).is_some());
        }
        omni_codes::validate().unwrap();
    }
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

impl std::fmt::Display for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let argv = self
            .prog
            .iter()
            .map(String::as_str)
            .chain(self.args.iter().map(String::as_str));
        f.write_str(&crate::display::format_argv(argv))
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
    resolve_command_with(spec, tera_ctx, env_vars, |s| s)
}

/// Like [`resolve_command`], but passes each rendered string through
/// `transform` *before* it is shell-split.
///
/// Redacting here (rather than on the split argv) keeps a value containing
/// whitespace matchable: once `shlex` splits `secret phrase` into two
/// arguments, neither half matches the original value.
pub fn resolve_command_with<F>(
    spec: &CommandConfig,
    tera_ctx: Option<&omni_tera::Context>,
    env_vars: Option<&Map<String, String>>,
    transform: F,
) -> Result<Command, ResolveError>
where
    F: for<'s> Fn(Cow<'s, str>) -> Cow<'s, str>,
{
    match spec {
        CommandConfig::Shell(s) => {
            let s = maybe_render(s, tera_ctx)?;
            let s = match env_vars {
                Some(vars) => Cow::Owned(::env::expand(s.as_ref(), vars)),
                None => s,
            };
            let s = transform(s);
            let argv = shlex::split(s.as_ref())
                .ok_or(ResolveError::InvalidShellSyntax)?;

            Ok(Command::from_argv(argv))
        }
        CommandConfig::Operands(items) => {
            let argv = items
                .iter()
                .map(|op| resolve_operand(op, tera_ctx, env_vars, &transform))
                .collect::<Result<Vec<String>, ResolveError>>()?;

            Ok(Command::from_argv(argv))
        }
        CommandConfig::Argv(items) => {
            let argv = items
                .iter()
                .map(|e| {
                    let e = maybe_render(e, tera_ctx)?;
                    let e = match env_vars {
                        Some(vars) => {
                            Cow::Owned(::env::expand(e.as_ref(), vars))
                        }
                        None => e,
                    };
                    Ok(transform(e).into_owned())
                })
                .collect::<Result<Vec<String>, ResolveError>>()?;

            Ok(Command::from_argv(argv))
        }
    }
}

fn expand_env<'a>(
    s: Cow<'a, str>,
    env_vars: Option<&Map<String, String>>,
) -> Cow<'a, str> {
    match env_vars {
        Some(vars) => Cow::Owned(::env::expand(s.as_ref(), vars)),
        None => s,
    }
}

fn resolve_operand<F>(
    op: &CommandOperand,
    tera_ctx: Option<&omni_tera::Context>,
    env_vars: Option<&Map<String, String>>,
    transform: &F,
) -> Result<String, ResolveError>
where
    F: for<'s> Fn(Cow<'s, str>) -> Cow<'s, str>,
{
    match op {
        CommandOperand::Literal(s) => {
            Ok(transform(Cow::Borrowed(s.as_str())).into_owned())
        }
        CommandOperand::Expression(s) => {
            let e = expand_env(maybe_render(s, tera_ctx)?, env_vars);
            Ok(transform(e).into_owned())
        }
        CommandOperand::Interpolated {
            expression,
            literals,
        } => {
            // Render with opaque placeholders, then put the real values in
            // after every interpretation step has run.
            let names: Vec<&String> = literals.keys().collect();
            let rendered = match tera_ctx {
                Some(base) => {
                    let mut ctx = base.clone();
                    for (i, name) in names.iter().enumerate() {
                        ctx.insert(name.to_string(), &placeholder(i));
                    }
                    omni_tera::one_off(expression, "command", &ctx)?
                }
                None => expression.clone(),
            };
            let expanded = expand_env(Cow::Owned(rendered), env_vars);
            let values: Vec<&String> = literals.values().collect();
            let substituted = substitute_placeholders(&expanded, &values);
            Ok(transform(Cow::Owned(substituted)).into_owned())
        }
    }
}

const PLACEHOLDER_MARK: char = '\u{1}';

fn placeholder(index: usize) -> String {
    format!("{PLACEHOLDER_MARK}{index}{PLACEHOLDER_MARK}")
}

/// Single pass, so substituted values are never rescanned.
fn substitute_placeholders(s: &str, values: &[&String]) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find(PLACEHOLDER_MARK) {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let replaced = after.find(PLACEHOLDER_MARK).and_then(|end| {
            let idx = after[..end].parse::<usize>().ok()?;
            Some((values.get(idx)?, end))
        });
        match replaced {
            Some((value, end)) => {
                out.push_str(value);
                rest = &after[end + 1..];
            }
            None => {
                out.push(PLACEHOLDER_MARK);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
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

    #[test]
    fn display_joins_and_quotes_argv() {
        let command = Command {
            prog: Some("echo".to_string()),
            args: vec!["hello world".to_string(), "plain".to_string()],
        };
        assert_eq!(command.to_string(), "echo 'hello world' plain");
    }

    #[test]
    fn display_of_empty_is_empty() {
        assert_eq!(Command::default().to_string(), "");
    }

    const NASTY: &[&str] = &[
        "$VAR",
        "${VAR}",
        "$(touch /tmp/omni-r03-pwned)",
        "{{ x }}",
        "{% if true %}y{% endif %}",
        "{# c #}",
        "a\\b\\n",
        "--flag=value",
        "-rf",
        "a b 'c' \"d\"",
        "x;y|z&&w",
        "",
    ];

    fn lit_resolve(v: &str, ctx: bool) -> Command {
        let mut c = Context::new();
        c.insert("x", "RENDERED");
        let e = env(&[("VAR", "EXPANDED")]);
        resolve_command(
            &CommandConfig::operands([
                CommandOperand::literal("echo"),
                CommandOperand::literal(v),
            ]),
            ctx.then_some(&c),
            Some(&e),
        )
        .unwrap()
    }

    #[test]
    fn literal_is_never_interpreted() {
        for v in NASTY {
            for ctx in [true, false] {
                let r = lit_resolve(v, ctx);
                assert_eq!(
                    r.to_argv(),
                    vec!["echo".to_string(), v.to_string()]
                );
            }
        }
    }

    #[test]
    fn same_text_expression_is_interpreted() {
        let mut c = Context::new();
        c.insert("x", "RENDERED");
        let e = env(&[("VAR", "EXPANDED")]);
        let r = resolve_command(
            &CommandConfig::operands([
                CommandOperand::expression("echo"),
                CommandOperand::expression("$VAR"),
                CommandOperand::expression("{{ x }}"),
            ]),
            Some(&c),
            Some(&e),
        )
        .unwrap();
        assert_eq!(r.to_argv(), vec!["echo", "EXPANDED", "RENDERED"]);
    }

    #[test]
    fn literal_with_spaces_is_not_split() {
        let r = lit_resolve("a b", false);
        assert_eq!(r.args, vec!["a b"]);
    }

    #[test]
    fn literal_is_still_redacted_by_transform() {
        let r = resolve_command_with(
            &CommandConfig::operands([
                CommandOperand::literal("echo"),
                CommandOperand::literal("secret"),
            ]),
            None,
            None,
            |s| Cow::Owned(s.replace("secret", "***")),
        )
        .unwrap();
        assert_eq!(r.args, vec!["***"]);
    }

    #[test]
    fn literal_display_shows_real_argv() {
        let r = lit_resolve("$(rm -rf /)", false);
        assert_eq!(r.to_string(), "echo '$(rm -rf /)'");
    }

    #[test]
    fn interpolated_protects_bound_data_in_authored_expression() {
        // C16: inferred data inside an authored expression.
        let mut c = Context::new();
        c.insert("other", "O");
        let e = env(&[("HOME", "/home/u")]);
        for v in NASTY {
            let r = resolve_command(
                &CommandConfig::operands([
                    CommandOperand::literal("run"),
                    CommandOperand::interpolated(
                        "$HOME/{{ other }}/{{ name }}/{{ name }}",
                        [("name", *v)],
                    ),
                ]),
                Some(&c),
                Some(&e),
            )
            .unwrap();
            assert_eq!(r.args, vec![format!("/home/u/O/{v}/{v}")]);
        }
    }

    #[test]
    fn interpolated_value_containing_placeholder_is_not_rescanned() {
        let r = resolve_command(
            &CommandConfig::operands([CommandOperand::interpolated(
                "{{ a }}{{ b }}",
                [("a", "\u{1}1\u{1}"), ("b", "B")],
            )]),
            Some(&Context::new()),
            None,
        )
        .unwrap();
        assert_eq!(r.prog.as_deref(), Some("\u{1}1\u{1}B"));
    }
}
