use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

#[derive(Debug, thiserror::Error, new)]
#[error(transparent)]
pub struct ServiceError(pub(crate) ServiceErrorInner);

impl ServiceError {
    #[inline(always)]
    pub fn custom(message: impl Into<String>) -> Self {
        Self(ServiceErrorInner::Custom(eyre::Report::msg(message.into())))
    }

    #[inline(always)]
    pub fn custom_error(error: impl Into<eyre::Report>) -> Self {
        Self(ServiceErrorInner::Custom(error.into()))
    }
}

impl ServiceError {
    #[allow(unused)]
    pub fn kind(&self) -> ServiceErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<ServiceErrorInner>> From<T> for ServiceError {
    fn from(inner: T) -> Self {
        let inner = inner.into();

        Self(inner)
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants, new)]
#[strum_discriminants(vis(pub), name(ServiceErrorKind))]
pub(crate) enum ServiceErrorInner {
    #[error(transparent)]
    Custom(#[from] eyre::Report),
}

pub type ServiceResult<T> = Result<T, ServiceError>;
