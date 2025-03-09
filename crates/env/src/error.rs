use derive_more::Constructor;

#[derive(Debug, thiserror::Error, Constructor)]
#[error(transparent)]
pub struct ParseError {
    #[from]
    repr: ParseErrorRepr,
}

impl ParseError {
    pub fn long_message(&self) -> Option<&str> {
        self.repr.long_message()
    }

    pub fn message(&self) -> &str {
        self.repr.message()
    }

    pub fn line(&self) -> usize {
        self.repr.line()
    }

    pub fn column(&self) -> usize {
        self.repr.column()
    }
}

impl ParseError {
    pub fn syntax(
        line: usize,
        column: usize,
        message: String,
        long_message: Option<String>,
    ) -> Self {
        Self::new(ParseErrorRepr::syntax(line, column, message, long_message))
    }
}

#[derive(Debug, thiserror::Error)]

pub enum ParseErrorRepr {
    #[error(transparent)]
    SyntaxError(#[from] SyntaxError),
}

impl ParseErrorRepr {
    pub fn syntax(
        line: usize,
        column: usize,
        message: String,
        long_message: Option<String>,
    ) -> Self {
        Self::SyntaxError(SyntaxError::new(line, column, message, long_message))
    }

    pub fn long_message(&self) -> Option<&str> {
        match self {
            Self::SyntaxError(e) => e.long_message.as_deref(),
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::SyntaxError(e) => e.message.as_str(),
        }
    }

    pub fn line(&self) -> usize {
        match self {
            Self::SyntaxError(e) => e.line,
        }
    }

    pub fn column(&self) -> usize {
        match self {
            Self::SyntaxError(e) => e.column,
        }
    }
}

#[derive(Debug, thiserror::Error, Constructor)]
#[error("SyntaxError: at line {line}, column {column}: {message}")]
pub struct SyntaxError {
    line: usize,
    column: usize,
    message: String,
    long_message: Option<String>,
}

pub type EnvParserResult<T> = Result<T, Vec<ParseError>>;
