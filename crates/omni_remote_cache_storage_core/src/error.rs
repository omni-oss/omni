use derive_new::new;
use strum::{
    EnumDiscriminants, EnumIter, IntoDiscriminant as _, VariantArray,
    VariantNames,
};

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct Error(pub(crate) ErrorInner);

impl Error {
    #[allow(unused)]
    pub fn kind(&self) -> ErrorKind {
        self.0.discriminant()
    }
}

impl Error {
    pub fn custom(error: impl Into<eyre::Report>) -> Self {
        Self(ErrorInner::Custom(error.into()))
    }
}

impl<T: Into<ErrorInner>> From<T> for Error {
    #[allow(unused)]
    #[inline(always)]
    fn from(inner: T) -> Self {
        let inner = inner.into();
        Self(inner)
    }
}

#[derive(Debug, new, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(
    vis(pub),
    name(ErrorKind),
    derive(EnumIter, VariantArray, VariantNames)
)]
pub(crate) enum ErrorInner {
    #[error(transparent)]
    Custom(eyre::Report),
}
