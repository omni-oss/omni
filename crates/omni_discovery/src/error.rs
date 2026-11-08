use std::path::PathBuf;

use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

omni_codes::prefixes!("discovery");

#[derive(thiserror::Error, Debug, omni_diagnostic::Diagnostic)]
#[error(transparent)]
#[diagnostic(forward)]
pub struct Error(pub(crate) ErrorInner);

impl Error {
    #[allow(unused)]
    pub fn kind(&self) -> ErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<ErrorInner>> From<T> for Error {
    fn from(value: T) -> Self {
        let inner = value.into();
        Self(inner)
    }
}

#[derive(
    thiserror::Error, Debug, EnumDiscriminants, new, omni_diagnostic::Diagnostic,
)]
#[strum_discriminants(vis(pub), name(ErrorKind))]
pub(crate) enum ErrorInner {
    #[error(transparent)]
    #[diagnostic(
        code = "discovery.invalid_glob",
        hint = "check the syntax of the configured file discovery patterns"
    )]
    /// A file discovery pattern is invalid.
    Glob(#[from] globset::Error),

    /// A directory could not be traversed during file discovery.
    #[error("failed to walk dir: {dir}")]
    #[diagnostic(code = "discovery.walk_failed", audience = environment, hint = "check that {dir:?} exists and that its directories are readable and searchable")]
    WalkDir {
        #[diagnostic(detail)]
        dir: PathBuf,
        #[new(into)]
        #[source]
        source: eyre::Report,
    },

    /// A directory entry could not be read during file discovery.
    #[error("failed to get dir entry")]
    #[diagnostic(code = "discovery.entry_unreadable", audience = environment, hint = "check filesystem access and whether discovered files were removed during traversal")]
    FailedToGetDirEntry {
        #[new(into)]
        #[source]
        source: eyre::Report,
    },

    #[error(transparent)]
    #[diagnostic(delegate)]
    Unknown(
        #[new(into)]
        #[from]
        eyre::Report,
    ),
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;
    use eyre::WrapErr;
    use omni_diagnostic::{Audience, find};

    #[test]
    fn discovery_failures_survive_public_wrappers_and_context() {
        let error = Error::from(ErrorInner::WalkDir {
            dir: PathBuf::from("private"),
            source: eyre::eyre!("permission denied"),
        });
        let report = Err::<(), _>(error)
            .wrap_err("discovering files")
            .unwrap_err();
        let diagnostic = find(&report).expect("discovery diagnostic");
        assert_eq!(diagnostic.code(), "discovery.walk_failed");
        assert_eq!(diagnostic.audience(), Audience::Environment);
        assert_eq!(diagnostic.details().unwrap()["dir"], "private");
        let nested: eyre::Report =
            Error::from(ErrorInner::Unknown(report)).into();
        assert_eq!(find(&nested).unwrap().code(), "discovery.walk_failed");
        let plain: eyre::Report =
            Error::from(ErrorInner::Unknown(eyre::eyre!("plain"))).into();
        assert!(find(&plain).is_none());
        for code in [
            "discovery.walk_failed",
            "discovery.invalid_glob",
            "discovery.entry_unreadable",
        ] {
            assert!(omni_codes::get(code).is_some());
        }
        omni_codes::validate().unwrap();
    }
}
