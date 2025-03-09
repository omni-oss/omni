mod error;

pub use crate::tokens::*;
use crate::utils::is_valid_identifier_char;
pub use error::*;

#[derive(Debug, Clone, PartialEq, Eq, Copy, Default)]
pub(crate) struct LexerOptions {
    pub stop_at_first_error: bool,
}

pub(crate) struct Lexer {
    source: String,
    current: usize,
    start: usize,
    line: usize,
    column: usize,
    options: LexerOptions,
}

impl Lexer {
    pub fn new(
        source: impl Into<String>,
        options: Option<LexerOptions>,
    ) -> Self {
        let source = source.into();
        Self {
            source,
            current: 0,
            start: 0,
            line: 1,
            column: 1,
            options: options.unwrap_or_default(),
        }
    }

    fn at_end(&self) -> bool {
        self.current >= self.source.len()
    }

    fn at_start(&self) -> bool {
        self.current == 0
    }

    fn err_reached_eof(&self) -> LexerError {
        LexerError {
            line: self.line,
            column: self.column,
            error_type: LexerErrorType::ReachedEof,
        }
    }

    fn err_unexpected_character(&self, character: char) -> LexerError {
        LexerError {
            line: self.line,
            column: self.column,
            error_type: LexerErrorType::UnexpectedCharacter(character),
        }
    }

    fn advance(&mut self) -> Result<(), LexerError> {
        if self.at_end() {
            return Err(self.err_reached_eof());
        }

        self.column += 1;
        if self.current() == Some('\n') {
            self.line += 1;
            self.column = 1;
        }

        self.current += 1;
        Ok(())
    }

    fn advance_until_char(&mut self, c: char) -> Result<(), LexerError> {
        self.advance_while(|x| x != c)
    }

    fn advance_while(
        &mut self,
        f: impl Fn(char) -> bool,
    ) -> Result<(), LexerError> {
        if self.at_end() {
            return Err(self.err_reached_eof());
        }

        while self.current().map(&f) == Some(true) {
            self.advance()?;

            if self.at_end() {
                return Err(self.err_reached_eof());
            }
        }

        Ok(())
    }

    fn get_until_char(&mut self, c: char) -> Result<String, LexerError> {
        self.get_while(|x| x != c)
    }

    fn get_until_quote(&mut self, quote: char) -> Result<String, LexerError> {
        if quote != '"' && quote != '\'' {
            panic!("Quote must be either ' or \"");
        }

        if self.at_end() {
            return Err(self.err_reached_eof());
        }

        let mut chars = Vec::new();
        while let Some(c) = self.current() {
            if c == quote && self.previous() != Some('\\') {
                break;
            }

            chars.push(c);
            self.advance()?;
        }

        Ok(chars.into_iter().collect())
    }

    fn get_while(
        &mut self,
        f: impl Fn(char) -> bool,
    ) -> Result<String, LexerError> {
        if self.at_end() {
            return Err(self.err_reached_eof());
        }

        let mut chars = Vec::new();
        while let Some(c) = self.current() {
            if !f(c) {
                break;
            }

            chars.push(c);
            self.advance()?;
        }

        Ok(chars.into_iter().collect())
    }

    fn iterate(&mut self) -> Result<char, LexerError> {
        if self.at_end() {
            return Err(self.err_reached_eof());
        }
        let c = self.current().expect("Should have current");
        self.advance()?;
        Ok(c)
    }

    fn current(&self) -> Option<char> {
        if self.at_end() {
            return None;
        }
        self.source.chars().nth(self.current)
    }

    fn previous(&self) -> Option<char> {
        if self.at_start() {
            return None;
        }

        self.source.chars().nth(self.current - 1)
    }

    // fn match_char(&mut self, c: char) -> Result<bool, LexerError> {
    //     Ok(if self.peek() == Some(c) {
    //         self.advance()?;
    //         true
    //     } else {
    //         false
    //     })
    // }

    fn reset(&mut self) {
        self.current = 0;
        self.start = 0;
        self.line = 1;
        self.column = 1;
    }

    fn new_token(&self, token_type: TokenType, lexeme: String) -> Token {
        Token::new(token_type, lexeme, self.line, self.column)
    }

    pub fn analyze(&mut self) -> Result<Vec<Token>, Vec<LexerError>> {
        self.reset();
        let mut tokens = Vec::new();
        let mut errors = Vec::new();

        let mut found_eq = false;

        while !self.at_end() {
            let curr = self.iterate().expect("Should be able to get next char");
            match curr {
                '=' => {
                    tokens.push(
                        self.new_token(TokenType::Equal, curr.to_string()),
                    );

                    found_eq = true;
                }
                '#' => {
                    // Ignore comments
                    let res = self.advance_until_char('\n');

                    if let Err(err) = res {
                        errors.push(err);
                        if self.options.stop_at_first_error {
                            return Err(errors);
                        }
                    }
                }
                '\n' => {
                    tokens
                        .push(self.new_token(TokenType::Eol, curr.to_string()));
                    found_eq = false;
                }
                // handling of quoted strings
                c @ ('"' | '\'') => {
                    let res = self.get_until_quote(c);

                    self.advance().expect("Should be able to advance");
                    match res {
                        Ok(s) => {
                            tokens.push(self.new_token(
                                if c == '"' {
                                    TokenType::DoubleQuotedString
                                } else {
                                    TokenType::SingleQuotedString
                                },
                                s,
                            ));
                        }
                        Err(err) => {
                            errors.push(err);
                            if self.options.stop_at_first_error {
                                return Err(errors);
                            }
                        }
                    }
                }
                // handling of unquoted strings
                c if found_eq && !c.is_whitespace() => {
                    let res = self.get_until_char('\n');

                    match res {
                        Ok(s) => {
                            let mut s = &s[..];

                            'x: {
                                if let Some(idx) = s.find('#') {
                                    if idx == 0 {
                                        s = "";
                                        break 'x;
                                    }
                                    let c = s
                                        .chars()
                                        .nth(idx - 1)
                                        .expect("Should be able to get char");

                                    if c.is_whitespace() {
                                        s = &s[0..idx];
                                    }
                                }
                            }

                            tokens.push(self.new_token(
                                TokenType::UnqoutedString,
                                c.to_string() + s.trim(),
                            ));
                        }
                        Err(err) => {
                            errors.push(err);
                            if self.options.stop_at_first_error {
                                return Err(errors);
                            }
                        }
                    }
                }
                c if c.is_alphabetic() || c == '_' => {
                    let res = self.get_while(is_valid_identifier_char);

                    match res {
                        Ok(s) => {
                            tokens.push(self.new_token(
                                TokenType::Identifier,
                                c.to_string() + &s,
                            ));
                        }
                        Err(err) => {
                            errors.push(err);
                            if self.options.stop_at_first_error {
                                return Err(errors);
                            }
                        }
                    }
                }
                c if c.is_whitespace() => {
                    // Ignore whitespaces
                }
                _ => {
                    errors.push(self.err_unexpected_character(curr));
                    if self.options.stop_at_first_error {
                        return Err(errors);
                    }
                }
            }
        }

        tokens.push(Token::new(
            TokenType::Eof,
            "".to_string(),
            self.line + 1,
            self.column,
        ));

        Ok(tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lex_happy_path() {
        let mut lexer = Lexer::new("TEST=*VALUE!", None);
        let tokens = lexer.analyze().unwrap();
        assert_eq!(tokens.len(), 4);

        assert_eq!(tokens[0].token_type, TokenType::Identifier);
        assert_eq!(tokens[0].lexeme, "TEST");

        assert_eq!(tokens[1].token_type, TokenType::Equal);
        assert_eq!(tokens[1].lexeme, "=");

        assert_eq!(tokens[2].token_type, TokenType::UnqoutedString);
        assert_eq!(tokens[2].lexeme, "*VALUE!");

        assert_eq!(tokens[3].token_type, TokenType::Eof);
        assert_eq!(tokens[3].lexeme, "");
    }

    #[test]
    fn test_lex_with_whitespace() {
        let mut lexer = Lexer::new("    TEST  =   VALUE   ", None);
        let tokens = lexer.analyze().unwrap();
        assert_eq!(tokens.len(), 4);

        assert_eq!(tokens[0].token_type, TokenType::Identifier);
        assert_eq!(tokens[0].lexeme, "TEST");

        assert_eq!(tokens[1].token_type, TokenType::Equal);
        assert_eq!(tokens[1].lexeme, "=");

        assert_eq!(tokens[2].token_type, TokenType::UnqoutedString);
        assert_eq!(tokens[2].lexeme, "VALUE");

        assert_eq!(tokens[3].token_type, TokenType::Eof);
        assert_eq!(tokens[3].lexeme, "");
    }

    #[test]
    fn test_lex_quoted_strings_with_escapes() {
        const TESTDATA: &str = r#"
        TEST="VALUE\"\'"
        TEST2='VALUE2\"\''
        "#;
        let mut lexer = Lexer::new(TESTDATA, None);
        let tokens = lexer.analyze().unwrap();
        assert_eq!(tokens.len(), 10);

        assert_eq!(tokens[0].token_type, TokenType::Eol);
        assert_eq!(tokens[0].lexeme, "\n");

        assert_eq!(tokens[1].token_type, TokenType::Identifier);
        assert_eq!(tokens[1].lexeme, "TEST");

        assert_eq!(tokens[2].token_type, TokenType::Equal);
        assert_eq!(tokens[2].lexeme, "=");

        assert_eq!(tokens[3].token_type, TokenType::DoubleQuotedString);
        assert_eq!(tokens[3].lexeme, r#"VALUE\"\'"#);

        assert_eq!(tokens[4].token_type, TokenType::Eol);
        assert_eq!(tokens[4].lexeme, "\n");

        assert_eq!(tokens[5].token_type, TokenType::Identifier);
        assert_eq!(tokens[5].lexeme, "TEST2");

        assert_eq!(tokens[6].token_type, TokenType::Equal);
        assert_eq!(tokens[6].lexeme, "=");

        assert_eq!(tokens[7].token_type, TokenType::SingleQuotedString);
        assert_eq!(tokens[7].lexeme, r#"VALUE2\"\'"#);

        assert_eq!(tokens[8].token_type, TokenType::Eol);
        assert_eq!(tokens[8].lexeme, "\n");

        assert_eq!(tokens[9].token_type, TokenType::Eof);
        assert_eq!(tokens[9].lexeme, "");
    }

    #[test]
    fn test_lex_quoted_strings_with_newlines() {
        let mut lexer = Lexer::new("TEST=\"VALUE\nTEST\nANOTHER\"", None);
        let tokens = lexer.analyze().unwrap();
        assert_eq!(tokens.len(), 4);

        assert_eq!(tokens[0].token_type, TokenType::Identifier);
        assert_eq!(tokens[0].lexeme, "TEST");

        assert_eq!(tokens[1].token_type, TokenType::Equal);
        assert_eq!(tokens[1].lexeme, "=");

        assert_eq!(tokens[2].token_type, TokenType::DoubleQuotedString);
        assert_eq!(tokens[2].lexeme, "VALUE\nTEST\nANOTHER");

        assert_eq!(tokens[3].token_type, TokenType::Eof);
        assert_eq!(tokens[3].lexeme, "");
    }

    #[test]
    fn test_lex_eol() {
        let mut lexer = Lexer::new("\n", None);
        let tokens = lexer.analyze().unwrap();

        assert_eq!(tokens.len(), 2);

        assert_eq!(tokens[0].token_type, TokenType::Eol);
        assert_eq!(tokens[0].lexeme, "\n");
        assert_eq!(tokens[1].token_type, TokenType::Eof);
        assert_eq!(tokens[1].lexeme, "");
    }

    #[test]
    fn test_lex_comment() {
        let mut lexer = Lexer::new("# Test\nTEST=SOME VALUE", None);
        let tokens = lexer.analyze().unwrap();

        assert_eq!(tokens.len(), 5);

        assert_eq!(tokens[0].token_type, TokenType::Eol);
        assert_eq!(tokens[0].lexeme, "\n");

        assert_eq!(tokens[1].token_type, TokenType::Identifier);
        assert_eq!(tokens[1].lexeme, "TEST");

        assert_eq!(tokens[2].token_type, TokenType::Equal);
        assert_eq!(tokens[2].lexeme, "=");

        assert_eq!(tokens[3].token_type, TokenType::UnqoutedString);
        assert_eq!(tokens[3].lexeme, "SOME VALUE");
    }

    #[test]
    fn test_lex_inline_comment() {
        let mut lexer = Lexer::new("TEST=SOME VALUE #InlineComment", None);
        let tokens = lexer.analyze().unwrap();

        assert_eq!(tokens.len(), 4);

        assert_eq!(tokens[0].token_type, TokenType::Identifier);
        assert_eq!(tokens[0].lexeme, "TEST");

        assert_eq!(tokens[1].token_type, TokenType::Equal);
        assert_eq!(tokens[1].lexeme, "=");

        assert_eq!(tokens[2].token_type, TokenType::UnqoutedString);
        assert_eq!(tokens[2].lexeme, "SOME VALUE");
    }
}
