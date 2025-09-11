use std::{
    collections::HashMap,
    ffi::OsString,
    io::{BufReader, Read as _},
    path::Path,
    process::{Command, Stdio},
};

use derive_new::new;
use maps::Map;
use strum::{EnumDiscriminants, EnumIs, IntoDiscriminant as _};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FallbackMode {
    Unset,
    UnsetOrEmpty,
}

#[derive(Debug, Clone, PartialEq, new)]
pub enum ExpansionConfig {
    Variable {
        #[new(into)]
        key: String,
    },
    VariableWithFallback {
        #[new(into)]
        key: String,

        #[new(into)]
        fallback: String,

        #[new(into)]
        mode: FallbackMode,
    },
    #[allow(dead_code)]
    Command {
        #[new(into)]
        program: String,

        #[new(into)]
        args: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, new)]
struct Expansion {
    #[new(into)]
    pub(self) start_index: usize,
    #[new(into)]
    pub(self) end_index: usize,
    #[new(into)]
    pub(self) config: ExpansionConfig,
}

fn replace_at(
    text: &str,
    start_idx: usize,
    end_idx: usize,
    value: &str,
) -> String {
    let mut result = text.to_string();
    result.replace_range(start_idx..=end_idx, value);
    result
}

impl Expansion {
    pub fn expand(
        &self,
        text: &str,
        vars: &Map<String, String>,
        command_expansion_config: &CommandExpansionConfig,
    ) -> Result<String, ExpansionError> {
        let expanded: String;

        match self.config {
            ExpansionConfig::Variable { ref key } => {
                if let Some(value) = vars.get(key) {
                    expanded = replace_at(
                        text,
                        self.start_index,
                        self.end_index,
                        value,
                    );
                } else {
                    expanded = text.to_string();
                }
            }
            ExpansionConfig::VariableWithFallback {
                ref key,
                ref fallback,
                mode,
            } => {
                let value = vars.get(key);
                let expand_result: String;

                let allow_empty = mode == FallbackMode::UnsetOrEmpty;

                expanded = replace_at(
                    text,
                    self.start_index,
                    self.end_index,
                    if let Some(value) = value
                        && (if allow_empty { true } else { !value.is_empty() })
                    {
                        value
                    } else {
                        expand_result = expand(fallback, vars);
                        &expand_result
                    },
                );
            }
            ExpansionConfig::Command {
                ref program,
                ref args,
            } => match command_expansion_config {
                CommandExpansionConfig::Disabled => {
                    expanded = text.to_string();
                }
                CommandExpansionConfig::Enabled { cwd, env_vars } => {
                    let mut cmd = Command::new(program);

                    cmd.args(args)
                        .envs(*env_vars)
                        .current_dir(cwd)
                        .stdin(Stdio::null())
                        .stderr(Stdio::null())
                        .stdout(Stdio::piped());

                    let mut child = cmd.spawn()?;

                    let stdout = child.stdout.take();
                    if let Some(stdout) = stdout {
                        let mut reader = BufReader::new(stdout);
                        let mut text_output = String::new();
                        reader.read_to_string(&mut text_output)?;

                        expanded = text_output;
                    } else {
                        expanded = text.to_string();
                    }
                }
            },
        };

        Ok(expanded)
    }
}

#[derive(Debug, Clone, new)]
pub enum CommandExpansionConfig<'a> {
    Disabled,
    Enabled {
        #[new(into)]
        cwd: &'a Path,
        #[new(into)]
        env_vars: &'a HashMap<OsString, OsString>,
    },
}

impl Default for CommandExpansionConfig<'_> {
    fn default() -> Self {
        Self::Disabled
    }
}

#[derive(Debug, Clone, PartialEq, new)]
pub struct Expansions<'a> {
    #[new(into)]
    expansions: Vec<Expansion>,

    #[new(into)]
    text: &'a str,
}

impl<'a> Expansions<'a> {
    pub fn is_empty(&self) -> bool {
        self.expansions.is_empty()
    }

    pub fn expand(
        &self,
        envs: &Map<String, String>,
        command_config: &CommandExpansionConfig,
    ) -> Result<String, ExpansionError> {
        let mut expansions = self.expansions.clone();
        // from shortest to longest
        expansions.sort_by_key(|b| std::cmp::Reverse(b.start_index));

        let mut expanded = self.text.to_string();

        for expansion in expansions.iter() {
            expanded = expansion.expand(&expanded, envs, command_config)?;
        }

        Ok(expanded)
    }
}

#[derive(Debug)]
pub struct ExpansionParser<'a> {
    text: &'a str,
    chars: Vec<char>,
    pos: usize,
}

impl<'a> ExpansionParser<'a> {
    pub fn new(text: &'a str) -> Self {
        Self {
            text,
            chars: text.chars().collect(),
            pos: 0,
        }
    }

    fn at_end(&self) -> bool {
        self.pos >= self.text.len()
    }

    fn at_start(&self) -> bool {
        self.pos == 0
    }

    fn advance_by(&mut self, n: usize) -> Option<()> {
        for _ in 0..n {
            self.advance()?;
        }

        Some(())
    }

    fn char_at(&self, index: usize) -> Option<char> {
        self.chars.get(index).copied()
    }

    fn char_at_pos(&self) -> Option<char> {
        self.char_at(self.pos)
    }

    fn advance(&mut self) -> Option<char> {
        if self.at_end() {
            return None;
        }
        self.pos += 1;
        self.char_at_pos()
    }

    fn back(&mut self) -> Option<char> {
        if self.at_start() {
            return None;
        }
        self.pos -= 1;
        self.char_at_pos()
    }

    fn peek(&self) -> Option<char> {
        self.char_at(self.pos + 1)
    }

    fn match_char(&mut self, c: char) -> bool {
        self.match_when(|x| x == c)
    }

    fn match_when(&mut self, f: impl Fn(char) -> bool) -> bool {
        if let Some(c) = self.peek() {
            if f(c) {
                self.advance();
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    fn current(&self) -> Option<char> {
        if self.at_end() {
            return None;
        }
        self.char_at_pos()
    }

    fn get_while(
        &mut self,
        mut f: impl FnMut(char, Option<char>) -> bool,
    ) -> Option<String> {
        if self.at_end() {
            return None;
        }
        let mut chars = String::new();
        while let Some(c) = self.current() {
            if !f(c, self.peek_back()) {
                break;
            }
            chars.push(c);
            self.advance();
        }
        Some(chars)
    }

    fn try_parse_identifier(&mut self) -> Option<String> {
        let curr = self.current()?;

        if !curr.is_alphabetic() || curr == '_' {
            return None;
        }

        let key = self.get_while(|x, _| x.is_alphanumeric() || x == '_')?;

        Some(key)
    }

    fn peek_back(&self) -> Option<char> {
        if self.at_start() {
            return None;
        }
        self.char_at(self.pos - 1)
    }

    fn try_parse_variable_expansion(&mut self) -> Option<Expansion> {
        let start_pos = self.pos;

        if self.current()? != '$' {
            return None;
        }

        if !self.match_char('{') {
            self.advance()?;
            let ident = self.try_parse_identifier()?;
            self.back()?;
            let end_pos = self.pos;

            let config = ExpansionConfig::new_variable(&ident);
            return Some(Expansion::new(start_pos, end_pos, config));
        }

        self.advance()?;

        let key = self.try_parse_identifier()?;

        let curr = self.current()?;

        if !(curr == ':' || curr == '-') {
            if self.current() == Some('}') {
                let config = ExpansionConfig::new_variable(&key);
                let end_pos = self.pos;

                return Some(Expansion::new(start_pos, end_pos, config));
            } else {
                return None;
            }
        }

        let is_unset_or_empty = curr == ':';

        if (curr == ':' && self.peek()? == '-') || curr == '-' {
            self.advance_by(if curr == ':' { 2 } else { 1 })?;

            let mut bracket_nesting = 1;

            let fallback = self.get_while(|x, prev| {
                bracket_nesting += match x {
                    '{' if prev == Some('$') => 1,
                    '}' if bracket_nesting > 0 => -1,
                    _ => 0,
                };

                x != '}' || bracket_nesting != 0
            })?;

            if self.current() != Some('}') {
                return None;
            }

            let config = ExpansionConfig::new_variable_with_fallback(
                &key,
                &fallback,
                if is_unset_or_empty {
                    FallbackMode::UnsetOrEmpty
                } else {
                    FallbackMode::Unset
                },
            );

            let end_pos = self.pos;

            return Some(Expansion::new(start_pos, end_pos, config));
        }

        None
    }

    // parse syntax $(command ...args)
    fn try_parse_command_expansion(&mut self) -> Option<Expansion> {
        let start_pos = self.pos;

        if self.current()? != '$' {
            return None;
        }

        if !self.match_char('(') {
            return None;
        }

        if !self.at_end() {
            self.advance()?;
        }

        let mut acc = String::new();
        while let Some(c) = self.current() {
            if c == ')' {
                break;
            }

            acc.push(c);

            self.advance()?;
        }

        let end_pos = self.pos;
        self.advance();

        let command = shlex::split(&acc);
        if let Some(command) = command
            && !command.is_empty()
        {
            let program =
                command.first().expect("should have at least one element");

            let args = command.iter().skip(1).cloned().collect::<Vec<_>>();

            let config = ExpansionConfig::new_command(program, args);
            let expansion = Expansion::new(start_pos, end_pos, config);

            Some(expansion)
        } else {
            None
        }
    }

    fn set_pos(&mut self, pos: usize) {
        self.pos = pos;
    }

    fn count_dollar_signs(&mut self) -> usize {
        self.chars.iter().filter(|c| **c == '$').count()
    }

    pub fn parse(&mut self) -> Expansions<'a> {
        // optimization: preallocate the vector with the number of dollar signs
        let dollar_signs_count = self.count_dollar_signs();
        let mut expansions = Vec::with_capacity(dollar_signs_count);

        while let Some(c) = self.current() {
            if c == '$' && self.peek_back() != Some('\\') {
                let current_pos = self.pos;
                if self.peek() == Some('{')
                    || self
                        .peek()
                        .map(|x| x.is_alphabetic() || x == '_')
                        .unwrap_or(false)
                {
                    let ex = self.try_parse_variable_expansion();
                    if let Some(ex) = ex {
                        expansions.push(ex);
                    } else {
                        self.set_pos(current_pos);
                    }
                } else if self.peek() == Some('(') {
                    let ex = self.try_parse_command_expansion();
                    if let Some(ex) = ex {
                        expansions.push(ex);
                    } else {
                        self.set_pos(current_pos);
                    }
                }
            }

            self.advance();
        }

        Expansions::new(expansions, self.text)
    }
}

fn replace_escaped(text: &str) -> String {
    text.replace(r"\$", "$")
}

pub fn expand_with_command_config(
    str: &str,
    envs: &Map<String, String>,
    command_config: &CommandExpansionConfig,
) -> Result<String, ExpansionError> {
    let parsed = ExpansionParser::new(str).parse();

    // short circuit if there are no expansions
    if parsed.is_empty() {
        return Ok(replace_escaped(str));
    }

    let expanded = parsed.expand(envs, command_config)?;

    Ok(replace_escaped(&expanded))
}

pub fn expand_into_with_command_config(
    into: &mut Map<String, String>,
    using: &Map<String, String>,
    command_config: &CommandExpansionConfig,
) -> Result<(), ExpansionError> {
    let mut using = using.clone();
    for (key, value) in into.iter_mut() {
        *value = expand_with_command_config(value, &using, command_config)?;

        using.insert(key.clone(), value.clone());
    }

    Ok(())
}

#[inline(always)]
pub fn expand(str: &str, envs: &Map<String, String>) -> String {
    expand_with_command_config(str, envs, &CommandExpansionConfig::default())
        .expect("should have no error at this point")
}

#[inline(always)]
pub fn expand_into(
    into: &mut Map<String, String>,
    using: &Map<String, String>,
) {
    expand_into_with_command_config(
        into,
        using,
        &CommandExpansionConfig::default(),
    )
    .expect("should have no error at this point")
}

#[derive(Debug, thiserror::Error)]
#[error("{inner}")]
pub struct ExpansionError {
    kind: ExpansionErrorKind,
    inner: ExpansionErrorInner,
}

impl<T: Into<ExpansionErrorInner>> From<T> for ExpansionError {
    fn from(inner: T) -> Self {
        let inner = inner.into();
        let kind = inner.discriminant();

        Self { kind, inner }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(vis(pub), name(ExpansionErrorKind), derive(EnumIs))]
pub enum ExpansionErrorInner {
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand() {
        let text = r#"${TEST}"#;

        let envs = maps::map![
            "TEST".to_string() => "TEST_VALUE".to_string()
        ];
        let expanded = expand(text, &envs);

        assert_eq!(expanded, "TEST_VALUE");
    }

    #[test]
    fn test_expand_simple_variable() {
        let text = r#"   $TEST   "#;

        let envs = maps::map![
            "TEST".to_string() => "TEST_VALUE".to_string()
        ];
        let expanded = expand(text, &envs);

        assert_eq!(expanded, "   TEST_VALUE   ");
    }

    #[test]
    fn test_expand_multiple_variable() {
        let text = r#"$TES_-$TEST1-$TEST2-${TEST3}-${TEST4:-DEFAULT_VALUE{}}"#;

        let envs = maps::map![
            "TES_".to_string() => "TES_VALUE".to_string(),
            "TEST1".to_string() => "TEST_VALUE1".to_string(),
            "TEST2".to_string() => "TEST_VALUE2".to_string(),
            "TEST3".to_string() => "TEST_VALUE3".to_string(),
        ];
        let expanded = expand(text, &envs);

        assert_eq!(
            expanded,
            "TES_VALUE-TEST_VALUE1-TEST_VALUE2-TEST_VALUE3-DEFAULT_VALUE{}"
        );
    }

    #[test]
    fn test_expand_with_unset_fallback() {
        let text = r#"${TEST-DEFAULT_VALUE    }"#;

        let envs = maps::map![];
        let expanded = expand(text, &envs);

        assert_eq!(expanded, "DEFAULT_VALUE    ");
    }

    #[test]
    fn test_expand_with_unset_nested_fallback() {
        let text = r#"${TEST-${TEST2-${TEST3-DEFAULT_VALUE}}}"#;
        let envs = maps::map![];
        let expanded = expand(text, &envs);
        assert_eq!(expanded, "DEFAULT_VALUE");
    }

    #[test]
    fn test_expand_with_unset_or_empty_fallback() {
        let text = r#"${TEST:-DEFAULT_VALUE}"#;

        let envs = maps::map![];
        let expanded = expand(text, &envs);

        assert_eq!(expanded, "DEFAULT_VALUE");
    }

    #[test]
    fn test_expand_with_unset_or_empty_nested_fallback() {
        let text = r#"${TEST:-${TEST2:-${TEST3:-DEFAULT_VALUE}}}"#;

        let envs = maps::map![];
        let expanded = expand(text, &envs);

        assert_eq!(expanded, "DEFAULT_VALUE");
    }

    #[test]
    fn test_expand_into() {
        let envs = maps::map![
            "TEST_1".to_string() => "TEST_VALUE".to_string()
        ];
        let mut into = maps::map![
            "TEST".to_string() => "${TEST_1}".to_string()
        ];

        expand_into(&mut into, &envs);

        assert_eq!(into.get("TEST"), Some(&"TEST_VALUE".to_string()));
    }

    #[test]
    fn test_multiple_expansions_of_same_key() {
        let text = r#"${TEST}__${TEST}"#;

        let envs = maps::map![
            "TEST".to_string() => "TEST_VALUE".to_string()
        ];
        let expanded = expand(text, &envs);

        assert_eq!(expanded, "TEST_VALUE__TEST_VALUE");
    }

    #[test]
    fn test_escaped_expansion() {
        let text = r#"\${TEST}${TEST}"#;

        let envs = maps::map![
            "TEST".to_string() => "TEST_VALUE".to_string()
        ];
        let expanded = expand(text, &envs);

        assert_eq!(expanded, "${TEST}TEST_VALUE");
    }

    #[test]
    fn test_parse_command_expansion() {
        let text = r#"$(echo TEST_VALUE)"#;

        let mut parser = ExpansionParser::new(text);
        let expansion = parser.parse();

        assert_eq!(
            expansion.expansions.len(),
            1,
            "there should be one expansion"
        );
    }

    #[test]
    fn test_with_command_expansion_disabled() {
        let text = r#"$(echo TEST_VALUE)"#;

        let envs = maps::map![];

        let cmd_cfg = CommandExpansionConfig::new_disabled();
        let expanded = expand_with_command_config(text, &envs, &cmd_cfg)
            .expect("should run without error");

        assert_eq!(expanded, "$(echo TEST_VALUE)");
    }

    #[test]
    fn test_with_command_expansion_enabled() {
        let text = r#"$(echo TEST_VALUE)"#;

        let envs = maps::map![];

        let env_vars = HashMap::new();
        let cmd_cfg =
            CommandExpansionConfig::new_enabled(Path::new("."), &env_vars);

        let expanded = expand_with_command_config(text, &envs, &cmd_cfg)
            .expect("should be able to expand");

        assert_eq!(expanded, "TEST_VALUE\n");
    }
}
