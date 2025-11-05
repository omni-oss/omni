use derive_more::Constructor;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

#[derive(Debug, thiserror::Error, Constructor)]
#[error(transparent)]
pub struct ParseError(pub(crate) ParseErrorInner);

impl ParseError {
    #[allow(unused)]
    pub fn kind(&self) -> ParseErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<ParseErrorInner>> From<T> for ParseError {
    fn from(value: T) -> Self {
        let repr = value.into();
        Self(repr)
    }
}

impl ParseError {
    pub fn long_message(&self) -> Option<&str> {
        self.0.long_message()
    }

    pub fn message(&self) -> &str {
        self.0.message()
    }

    pub fn line(&self) -> usize {
        self.0.line()
    }

    pub fn column(&self) -> usize {
        self.0.column()
    }
}

impl ParseError {
    pub fn syntax(
        line: usize,
        column: usize,
        message: String,
        long_message: Option<String>,
    ) -> Self {
        Self(ParseErrorInner::syntax(line, column, message, long_message))
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(name(ParseErrorKind), vis(pub))]
pub enum ParseErrorInner {
    #[error(transparent)]
    Syntax(#[from] SyntaxError),
}
impl ParseErrorInner {
    pub fn syntax(
        line: usize,
        column: usize,
        message: String,
        long_message: Option<String>,
    ) -> Self {
        Self::Syntax(SyntaxError::new(line, column, message, long_message))
    }
}

impl ParseErrorInner {
    pub fn long_message(&self) -> Option<&str> {
        match self {
            Self::Syntax(e) => e.long_message.as_deref(),
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::Syntax(e) => e.message.as_str(),
        }
    }

    pub fn line(&self) -> usize {
        match self {
            Self::Syntax(e) => e.line,
        }
    }

    pub fn column(&self) -> usize {
        match self {
            Self::Syntax(e) => e.column,
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
