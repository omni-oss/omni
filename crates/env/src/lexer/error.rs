use strum::{EnumDiscriminants, IntoDiscriminant as _};

#[derive(thiserror::Error, Debug)]
#[error("LexerError: {inner} at line {line}, column {column}")]
pub struct LexerError {
    #[source]
    inner: LexerErrorInner,
    kind: LexerErrorKind,
    line: usize,
    column: usize,
}

impl LexerError {
    pub fn new(inner: LexerErrorInner, line: usize, column: usize) -> Self {
        (inner, line, column).into()
    }
}

impl LexerError {
    pub(crate) fn inner(&self) -> &LexerErrorInner {
        &self.inner
    }

    #[allow(unused)]
    pub fn kind(&self) -> LexerErrorKind {
        self.kind
    }

    pub fn line(&self) -> usize {
        self.line
    }

    pub fn column(&self) -> usize {
        self.column
    }
}

impl<T: Into<LexerErrorInner>> From<(T, usize, usize)> for LexerError {
    fn from((inner, line, column): (T, usize, usize)) -> Self {
        let inner = inner.into();
        let kind = inner.discriminant();
        Self {
            inner,
            kind,
            line,
            column,
        }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(name(LexerErrorKind), vis(pub))]
pub(crate) enum LexerErrorInner {
    #[error("unexpected character '{0}'")]
    UnexpectedCharacter(char),
    // #[error("Unterminated string")]
    // UnterminatedString { token: Token },
    #[error("reached end of file")]
    ReachedEof,
}
