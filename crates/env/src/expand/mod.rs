use derive_new::new;
use maps::Map;

#[derive(Debug, Clone, Copy, PartialEq)]
enum FallbackMode {
    Unset,
    UnsetOrEmpty,
}

#[derive(Debug, Clone, PartialEq, new)]
enum ExpansionConfig {
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
        command: String,

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
    pub fn expand(&self, text: &str, envs: &Map<String, String>) -> String {
        let expanded: String;

        match self.config {
            ExpansionConfig::Variable { ref key } => {
                if let Some(value) = envs.get(key) {
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
                let value = envs.get(key);
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
                        expand_result = expand(fallback, envs);
                        &expand_result
                    },
                );
            }
            ExpansionConfig::Command { .. } => {
                unimplemented!(
                    "command expansion is not implemented yet. use variable expansion instead"
                )
            }
        };

        expanded
    }
}

#[derive(Debug, Clone, PartialEq, new)]
struct Expansions<'a> {
    #[new(into)]
    expansions: Vec<Expansion>,

    #[new(into)]
    text: &'a str,
}

impl<'a> Expansions<'a> {
    pub fn is_empty(&self) -> bool {
        self.expansions.is_empty()
    }

    pub fn expand(&self, envs: &Map<String, String>) -> String {
        let mut expansions = self.expansions.clone();
        // from shortest to longest
        expansions.sort_by_key(|b| std::cmp::Reverse(b.start_index));

        let mut expanded = self.text.to_string();

        for expansion in expansions.iter() {
            println!("expansion: {expansion:?}");
            expanded = expansion.expand(&expanded, envs);
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

    fn set_pos(&mut self, pos: usize) {
        self.pos = pos;
    }

    pub fn parse(&mut self) -> Expansions<'a> {
        let mut expansions = Vec::new();

        while let Some(c) = self.current() {
            if c == '$'
                && (self.peek() == Some('{')
                    || self
                        .peek()
                        .map(|x| x.is_alphabetic() || x == '_')
                        .unwrap_or(false))
                && self.peek_back() != Some('\\')
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

        Expansions::new(expansions, self.text)
    }
}

fn replace_escaped(text: &str) -> String {
    text.replace(r"\$", "$")
}

pub fn expand(str: &str, envs: &Map<String, String>) -> String {
    let parsed = ExpansionParser::new(str).parse();

    // short circuit if there are no expansions
    if parsed.is_empty() {
        return replace_escaped(str);
    }

    let expanded = parsed.expand(envs);

    replace_escaped(&expanded)
}

pub fn expand_into(
    into: &mut Map<String, String>,
    using: &Map<String, String>,
) {
    let mut using = using.clone();
    for (key, value) in into.iter_mut() {
        *value = expand(value, &using);

        using.insert(key.clone(), value.clone());
    }
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
}
