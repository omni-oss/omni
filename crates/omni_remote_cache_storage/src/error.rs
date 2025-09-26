use derive_new::new;
use strum::{
    EnumDiscriminants, EnumIter, IntoDiscriminant as _, VariantArray,
    VariantNames,
};

#[derive(Debug, thiserror::Error)]
#[error("Error: {inner:?}")]
pub struct Error {
    inner: ErrorInner,
    kind: ErrorKind,
}

impl Error {
    pub fn custom(error: impl Into<eyre::Report>) -> Self {
        Self {
            inner: ErrorInner::Custom(error.into()),
            kind: ErrorKind::Custom,
        }
    }
}

impl<T: Into<ErrorInner>> From<T> for Error {
    #[allow(unused)]
    #[inline(always)]
    fn from(inner: T) -> Self {
        let inner = inner.into();
        Self {
            kind: inner.discriminant(),
            inner,
        }
    }
}

#[derive(Debug, new, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(
    vis(pub),
    name(ErrorKind),
    derive(EnumIter, VariantArray, VariantNames)
)]
enum ErrorInner {
    #[error(transparent)]
    Custom(eyre::Report),
}
