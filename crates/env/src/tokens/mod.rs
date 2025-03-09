use derive_more::Constructor;

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub enum TokenType {
    // Symbols
    Equal,

    // Literals
    Identifier,

    UnqoutedString,
    SingleQuotedString,
    DoubleQuotedString,

    Eol,
    Eof,
}

#[derive(Debug, Clone, PartialEq, Eq, Constructor)]
pub struct Token {
    pub token_type: TokenType,
    pub lexeme: String,
    pub line: usize,
    pub column: usize,
}
