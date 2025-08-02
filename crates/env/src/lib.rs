mod config;
mod error;
mod escape;
mod expand;
mod lexer;
mod tokens;
mod utils;

pub use config::*;
pub use error::*;
use escape::unescape;
pub use expand::*;
use lexer::{Lexer, Token};
use maps::Map;

pub fn parse_default(text: &str) -> EnvParserResult<Map<String, String>> {
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
) -> EnvParserResult<Map<String, String>> {
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
                                        expand(&unescaped, &combined)
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
        return Err(stx_errors);
    }

    Ok(env)
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
