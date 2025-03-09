#[derive(thiserror::Error, Debug)]
#[error("LexerError: {error_type} at line {line}, column {column}")]
pub struct LexerError {
    pub error_type: LexerErrorType,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum LexerErrorType {
    #[error("Unexpected character '{0}'")]
    UnexpectedCharacter(char),
    // #[error("Unterminated string")]
    // UnterminatedString { token: Token },
    #[error("Reached end of file")]
    ReachedEof,
}
