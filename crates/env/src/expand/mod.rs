use std::collections::HashMap;

use derive_more::Constructor;

#[derive(Debug, Clone, Copy, PartialEq)]
enum FallbackMode {
    Unset,
    UnsetOrEmpty,
}

#[derive(Debug, Clone, PartialEq)]
enum ExpansionConfig {
    Variable {
        key: String,
    },
    VariableWithFallback {
        key: String,
        fallback: String,
        mode: FallbackMode,
    },
    #[allow(dead_code)]
    Command {
        command: String,
        args: Vec<String>,
    },
}

impl ExpansionConfig {
    pub fn new_variable(key: impl Into<String>) -> Self {
        Self::Variable { key: key.into() }
    }

    pub fn new_variable_with_fallback(
        key: impl Into<String>,
        fallback: impl Into<String>,
        mode: impl Into<FallbackMode>,
    ) -> Self {
        Self::VariableWithFallback {
            key: key.into(),
            fallback: fallback.into(),
            mode: mode.into(),
        }
    }

    #[allow(dead_code)]
    pub fn new_command(
        command: impl Into<String>,
        args: impl Into<Vec<String>>,
    ) -> Self {
        Self::Command {
            command: command.into(),
            args: args.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Constructor)]
pub struct Expansion {
    to_replace: String,
    config: ExpansionConfig,
}

impl Expansion {
    pub fn key(&self) -> Option<&str> {
        match &self.config {
            ExpansionConfig::Variable { key } => Some(key),
            ExpansionConfig::VariableWithFallback { key, .. } => Some(key),
            ExpansionConfig::Command { .. } => None,
        }
    }

    pub fn expand(&self, text: &str, envs: &HashMap<String, String>) -> String {
        let mut expanded = text.to_string();
        let to_replace: &str = &self.to_replace;

        match self.config {
            ExpansionConfig::Variable { ref key } => {
                if let Some(value) = envs.get(key) {
                    expanded = expanded.replacen(to_replace, value, 1);
                }
            }
            ExpansionConfig::VariableWithFallback {
                ref key,
                ref fallback,
                mode,
            } => {
                let value = envs.get(key);

                match mode {
                    FallbackMode::Unset => {
                        if let Some(value) = value {
                            expanded = expanded.replacen(to_replace, value, 1);
                        } else {
                            let fb = expand(fallback, envs);
                            expanded = expanded.replacen(to_replace, &fb, 1);
                        }
                    }
                    FallbackMode::UnsetOrEmpty => {
                        expanded = if let Some(value) = value
                            && !value.is_empty()
                        {
                            expanded.replacen(to_replace, value, 1)
                        } else {
                            let fb = expand(fallback, envs);
                            expanded.replacen(to_replace, &fb, 1)
                        };
                    }
                }
            }
            ExpansionConfig::Command { .. } => {
                unimplemented!("Command expansion")
            }
        };

        if expanded.contains(to_replace) {
            expanded = expanded.replacen(to_replace, "", 1);
        }

        expanded
    }
}

struct ExpansionParser<'a> {
    text: &'a str,
    pos: usize,
}

impl<'a> ExpansionParser<'a> {
    pub fn new(text: &'a str) -> Self {
        Self { text, pos: 0 }
    }

    fn at_end(&self) -> bool {
        self.pos >= self.text.len()
    }

    fn at_start(&self) -> bool {
        self.pos == 0
    }

    fn pos_clamped(&self) -> usize {
        self.pos.clamp(0, self.text.len())
    }

    fn advance_by(&mut self, n: usize) -> Option<()> {
        for _ in 0..n {
            self.advance()?;
        }

        Some(())
    }

    fn advance(&mut self) -> Option<char> {
        if self.at_end() {
            return None;
        }
        self.pos += 1;
        self.text.chars().nth(self.pos)
    }

    fn back(&mut self) -> Option<char> {
        if self.at_start() {
            return None;
        }
        self.pos -= 1;
        self.text.chars().nth(self.pos)
    }

    fn peek(&self) -> Option<char> {
        self.text.chars().nth(self.pos + 1)
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
        self.text.chars().nth(self.pos)
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
        self.text.chars().nth(self.pos - 1)
    }

    fn try_parse_variable_expansion(&mut self) -> Option<Expansion> {
        let current_pos = self.pos;

        if self.current()? != '$' {
            return None;
        }

        if !self.match_char('{') {
            self.advance()?;
            let ident = self.try_parse_identifier()?;
            let replace = &self.text[current_pos..self.pos_clamped()];
            self.back()?;

            let config = ExpansionConfig::new_variable(&ident);
            return Some(Expansion::new(replace.to_string(), config));
        }

        self.advance()?;

        let key = self.try_parse_identifier()?;

        let curr = self.current()?;

        if !(curr == ':' || curr == '-') {
            if self.current() == Some('}') {
                let config = ExpansionConfig::new_variable(&key);
                let to_replace = &self.text[current_pos..self.pos + 1];

                return Some(Expansion::new(to_replace.to_string(), config));
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

            let to_replace = self.text[current_pos..self.pos + 1].to_string();

            return Some(Expansion::new(to_replace, config));
        }

        None
    }

    fn set_pos(&mut self, pos: usize) {
        self.pos = pos;
    }

    pub fn parse(&mut self) -> Vec<Expansion> {
        let mut expansions = Vec::new();

        while let Some(c) = self.current() {
            if c == '$'
                && (self.peek() == Some('{')
                    || self
                        .peek()
                        .map(|x| x.is_alphabetic() || x == '_')
                        .unwrap_or(false))
            {
                let current_pos = self.pos;
                let ex = self.try_parse_variable_expansion();
                if let Some(ex) = ex {
                    expansions.push(ex);
                } else {
                    self.set_pos(current_pos);
                }
            }

            self.advance();
        }

        expansions
    }
}

fn get_expansions(text: &str) -> Vec<Expansion> {
    let mut parser = ExpansionParser::new(text);

    parser.parse()
}

pub fn expand(str: &str, envs: &HashMap<String, String>) -> String {
    let mut expanded = str.to_string();
    let mut parsed = get_expansions(str);

    parsed.sort_by(|a, b| b.to_replace.len().cmp(&a.to_replace.len()));

    for expansion in parsed {
        expanded = expansion.expand(&expanded, envs);
    }

    expanded
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_expand() {
        let text = r#"${TEST}"#;

        let mut envs = HashMap::new();
        envs.insert("TEST".to_string(), "TEST_VALUE".to_string());
        let expanded = expand(text, &envs);

        assert_eq!(expanded, "TEST_VALUE");
    }

    #[test]
    fn test_expand_simple_variable() {
        let text = r#"   $TEST   "#;

        let mut envs = HashMap::new();
        envs.insert("TEST".to_string(), "TEST_VALUE".to_string());
        let expanded = expand(text, &envs);

        assert_eq!(expanded, "   TEST_VALUE   ");
    }

    #[test]
    fn test_expand_multiple_variable() {
        let text = r#"$TES$TEST1$TEST2${TEST3}${TEST4:-DEFAULT_VALUE{}}"#;

        let mut envs = HashMap::new();
        envs.insert("TES".to_string(), "TES_VALUE".to_string());
        envs.insert("TEST1".to_string(), "TEST_VALUE1".to_string());
        envs.insert("TEST2".to_string(), "TEST_VALUE2".to_string());
        envs.insert("TEST3".to_string(), "TEST_VALUE3".to_string());
        let expanded = expand(text, &envs);

        assert_eq!(
            expanded,
            "TES_VALUETEST_VALUE1TEST_VALUE2TEST_VALUE3DEFAULT_VALUE{}"
        );
    }

    #[test]
    fn test_expand_with_unset_fallback() {
        let text = r#"${TEST-DEFAULT_VALUE    }"#;

        let envs = HashMap::new();
        let expanded = expand(text, &envs);

        assert_eq!(expanded, "DEFAULT_VALUE    ");
    }

    #[test]
    fn test_expand_with_notempty_fallback() {
        let text = r#"${TEST:-DEFAULT_VALUE}"#;

        let mut envs = HashMap::new();
        envs.insert("TEST".to_string(), "".to_string());
        let expanded = expand(text, &envs);

        assert_eq!(expanded, "DEFAULT_VALUE");
    }

    #[test]
    fn test_expand_with_notempty_nested_fallback() {
        let text = r#"${TEST:-${TEST2:-${TEST3:-DEFAULT_VALUE}}}"#;

        let mut envs = HashMap::new();
        envs.insert("TEST".to_string(), "".to_string());
        let expanded = expand(text, &envs);

        assert_eq!(expanded, "DEFAULT_VALUE");
    }
}
