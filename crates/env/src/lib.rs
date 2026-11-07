#![allow(clippy::redundant_field_names)]

mod config;
mod error;
mod escape;
mod expand;
mod lexer;
mod tokens;
mod utils;

#[cfg(test)]
mod diagnostic_tests;

pub use config::*;
use derive_new::new;
pub use error::*;
use escape::unescape;
pub use expand::*;
use lexer::{Lexer, Token};
use maps::Map;
use omni_diagnostic::__private::serde_json::{self, json};
use strum::{EnumDiscriminants, EnumIs, IntoDiscriminant as _};

pub fn parse_default(text: &str) -> Result<Map<String, String>, EnvParseError> {
    parse(text, &ParseConfig::default())
}

fn report_long_message(
    source: &str,
    line: usize,
    column: usize,
    message: &str,
) -> String {
    let line = source.lines().nth(line);

    let line = match line {
        Some(line) => line,
        None => return "".to_string(),
    };

    let diagnostic = " ".repeat(column - 1) + "^" + "\n" + message;

    line.to_owned() + "\n" + &diagnostic
}

pub fn parse(
    text: &str,
    config: &ParseConfig,
) -> Result<Map<String, String>, EnvParseError> {
    let n_lines = text
        .lines()
        .filter(|s| {
            let trimmed = s.trim();
            !trimmed.is_empty() && !trimmed.starts_with('#')
        })
        .count();

    let mut env = maps::map!(cap: n_lines);

    let mut combined = if let Some(extra_envs) = config.extra_envs {
        let mut map = maps::map!(cap: extra_envs.len() + n_lines);

        map.extend(extra_envs.clone());

        map
    } else {
        maps::map!(cap: n_lines)
    };

    let mut lexer = Lexer::new(text, None);

    let result = lexer.analyze();

    let mut key: Option<&Token> = None;
    let mut eq: Option<&Token> = None;
    let mut val: Option<&Token> = None;

    let mut stx_errors: Vec<ParseError> = Vec::new();

    let cmd_config_def = CommandExpansionConfig::default();
    let cmd_config = config.command_expand.unwrap_or(&cmd_config_def);

    match result {
        Ok(tokens) => {
            for token in &tokens {
                match token.token_type {
                    lexer::TokenType::Equal => {
                        eq = Some(token);
                    }
                    lexer::TokenType::Identifier => {
                        key = Some(token);
                    }
                    lexer::TokenType::UnqoutedString
                    | lexer::TokenType::SingleQuotedString
                    | lexer::TokenType::DoubleQuotedString => {
                        val = Some(token);
                    }
                    lexer::TokenType::Eol | lexer::TokenType::Eof => {
                        // We're matching only key and eq here because value may be empty
                        match (key, eq) {
                            (Some(ident), Some(_)) => {
                                if let Some(string) = val {
                                    let unescaped = unescape(&string.lexeme);
                                    let expanded = if config.expand
                                        && matches!(string.token_type,lexer::TokenType::UnqoutedString | lexer::TokenType::DoubleQuotedString
                                ) {
                                        expand_with_command_config(&unescaped, &combined, cmd_config)?
                                    } else {
                                        unescaped
                                    };

                                    env.insert(
                                        ident.lexeme.clone(),
                                        expanded.clone(),
                                    );
                                    combined
                                        .insert(ident.lexeme.clone(), expanded);
                                } else {
                                    env.insert(
                                        ident.lexeme.clone(),
                                        "".to_string(),
                                    );
                                    combined.insert(
                                        ident.lexeme.clone(),
                                        "".to_string(),
                                    );
                                }
                            }
                            (None, Some(eq)) => {
                                let repr: ParseErrorInner = SyntaxError::new(
                                    eq.line,
                                    eq.column,
                                    "Expected identifier".to_string(),
                                    Some(report_long_message(
                                        text,
                                        eq.line,
                                        eq.column,
                                        "Expected identifier",
                                    )),
                                )
                                .into();
                                stx_errors.push(repr.into());
                            }
                            (Some(ident), None) => {
                                let repr: ParseErrorInner = SyntaxError::new(
                                    ident.line,
                                    ident.column,
                                    "Expected '='".to_string(),
                                    Some(report_long_message(
                                        text,
                                        ident.line,
                                        ident.column,
                                        "Expected '='",
                                    )),
                                )
                                .into();

                                stx_errors.push(repr.into());
                            }
                            (None, None) => {
                                if let Some(string) = val {
                                    let repr: ParseErrorInner =
                                        SyntaxError::new(
                                            string.line,
                                            string.column,
                                            "Expected identifier".to_string(),
                                            Some(report_long_message(
                                                text,
                                                string.line,
                                                string.column,
                                                "Expected identifier",
                                            )),
                                        )
                                        .into();

                                    stx_errors.push(repr.into());
                                }
                            }
                        }

                        // Reset the state
                        val = None;
                        eq = None;
                        key = None;
                    }
                }
            }
        }
        Err(errors) => {
            for error in errors {
                let err_string = error.inner().to_string();
                let long = Some(report_long_message(
                    text,
                    error.line(),
                    error.column(),
                    &err_string,
                ));
                let parse_error = ParseError::syntax(
                    error.line(),
                    error.column(),
                    err_string,
                    long,
                );

                stx_errors.push(parse_error);
            }
        }
    }

    if !stx_errors.is_empty() {
        Err(EnvParseErrorInner::new_parse(stx_errors))?;
    }

    Ok(env)
}

omni_codes::prefixes!("env_parse");

#[derive(thiserror::Error, omni_diagnostic::Diagnostic)]
#[error(transparent)]
#[diagnostic(forward)]
pub struct EnvParseError(pub(crate) EnvParseErrorInner);

impl EnvParseError {
    pub fn kind(&self) -> EnvParseErrorKind {
        self.0.discriminant()
    }

    pub fn location(&self) -> Option<(usize, usize)> {
        self.0.first_location()
    }
}

impl std::fmt::Debug for EnvParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnvParseError")
            .field("kind", &self.kind())
            .field("inner", &self.0)
            .finish()
    }
}

impl<T: Into<EnvParseErrorInner>> From<T> for EnvParseError {
    fn from(inner: T) -> Self {
        Self(inner.into())
    }
}

#[derive(
    Debug, thiserror::Error, EnumDiscriminants, new, omni_diagnostic::Diagnostic,
)]
#[strum_discriminants(vis(pub), name(EnvParseErrorKind), derive(EnumIs))]
pub enum EnvParseErrorInner {
    #[error(transparent)]
    #[diagnostic(skip)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    #[diagnostic(skip)]
    Expansion(#[from] ExpansionError),

    /// The environment file has a syntax error.
    #[error("parser error: {first}", first = errors.first().unwrap())]
    #[diagnostic(
        code = "env_parse.syntax_error",
        details = syntax_details,
        hint = "check the env file syntax at the reported line and column"
    )]
    Parse { errors: Vec<ParseError> },
}

impl EnvParseErrorInner {
    fn first_location(&self) -> Option<(usize, usize)> {
        match self {
            Self::Parse { errors } => {
                errors.first().map(|e| (e.line(), e.column()))
            }
            _ => None,
        }
    }
}

fn syntax_details(e: &EnvParseErrorInner) -> serde_json::Value {
    match e.first_location() {
        Some((line, column)) => {
            json!({ "line": line, "column": column })
        }
        None => serde_json::Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse() {
        let text = r#"
            TEST=VALUE
            TEST2=VALUE2
            # Comment
            "#;
        let env = parse_default(text).unwrap();

        assert_eq!(env.get("TEST"), Some(&"VALUE".to_string()));
        assert_eq!(env.get("TEST2"), Some(&"VALUE2".to_string()));
    }

    #[test]
    fn test_parse_single_line() {
        let text = "TEST=VALUE";
        let env = parse_default(text).unwrap();

        assert_eq!(env.get("TEST"), Some(&"VALUE".to_string()));
    }

    #[test]
    fn test_interpolation() {
        let text = r#"
            # Comment
            TEST=VALUE
            TEST2=${TEST}-value

            "#;

        let env = parse_default(text).unwrap();
        assert_eq!(env.get("TEST2"), Some(&"VALUE-value".to_string()));
        assert_eq!(env.get("TEST"), Some(&"VALUE".to_string()));
    }

    #[test]
    fn test_interpolation_with_extra_envs() {
        let text = r#"
            TEST=VALUE
            TEST2=${EXTERNAL_TEST}-value

            # Comment
            "#;

        let env = parse(
            text,
            &ParseConfig {
                extra_envs: Some(&maps::map![
                    "EXTERNAL_TEST".to_string() => "EXTERNAL_VALUE".to_string(),
                    "EXTERNAL_TEST2".to_string() => "EXTERNAL_VALUE2".to_string(),
                ]),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(env.get("TEST2"), Some(&"EXTERNAL_VALUE-value".to_string()));
        assert_eq!(env.get("TEST"), Some(&"VALUE".to_string()));
    }
}
