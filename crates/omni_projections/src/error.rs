use strum::{EnumDiscriminants, IntoDiscriminant as _};

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct ProjectionError(pub(crate) ProjectionErrorInner);

impl ProjectionError {
    pub fn custom(message: impl Into<String>) -> Self {
        Self(ProjectionErrorInner::Custom(eyre::Report::msg(
            message.into(),
        )))
    }

    pub fn kind(&self) -> ProjectionErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<ProjectionErrorInner>> From<T> for ProjectionError {
    fn from(inner: T) -> Self {
        Self(inner.into())
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(vis(pub), name(ProjectionErrorKind))]
pub(crate) enum ProjectionErrorInner {
    #[error(transparent)]
    Custom(#[from] eyre::Report),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Hasher(#[from] omni_hasher::HasherError),
}

pub type Result<T, E = ProjectionError> = std::result::Result<T, E>;
