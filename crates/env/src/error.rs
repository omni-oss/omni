use derive_more::Constructor;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

#[derive(Debug, thiserror::Error, Constructor)]
#[error("ParseError: {inner}")]
pub struct ParseError {
    #[source]
    inner: ParseErrorInner,
    kind: ParseErrorKind,
}

impl<T: Into<ParseErrorInner>> From<T> for ParseError {
    fn from(value: T) -> Self {
        let repr = value.into();
        let kind = repr.discriminant();
        Self { inner: repr, kind }
    }
}

impl ParseError {
    pub fn long_message(&self) -> Option<&str> {
        self.inner.long_message()
    }

    pub fn message(&self) -> &str {
        self.inner.message()
    }

    pub fn line(&self) -> usize {
        self.inner.line()
    }

    pub fn column(&self) -> usize {
        self.inner.column()
    }
}

impl ParseError {
    pub fn syntax(
        line: usize,
        column: usize,
        message: String,
        long_message: Option<String>,
    ) -> Self {
        Self {
            kind: ParseErrorKind::Syntax,
            inner: ParseErrorInner::syntax(line, column, message, long_message),
        }
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
