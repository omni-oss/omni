use std::path::PathBuf;

/// Why a configured ignore file could not be parsed as having at most one
/// well-formed managed region.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum FenceError {
    #[error("found {0} managed begin markers; expected at most one")]
    DuplicateBegin(usize),

    #[error("found {0} managed end markers; expected at most one")]
    DuplicateEnd(usize),

    #[error("found a managed begin marker with no matching end marker")]
    UnclosedBegin,

    #[error("found a managed end marker with no preceding begin marker")]
    UnopenedEnd,

    #[error("the managed end marker appears before the begin marker")]
    CrossedFence,
}

/// Errors raised while building patterns or patching ignore files.
#[derive(thiserror::Error, Debug)]
pub enum IgnoreError {
    #[error("ignore pattern is empty")]
    EmptyPattern,

    #[error(
        "ignore pattern must be a single line but contains a line break: {0:?}"
    )]
    MultiLinePattern(String),

    #[error("ignore pattern collides with a managed fence marker: {0:?}")]
    FenceInPattern(String),

    #[error("{path}: {source}")]
    MalformedFence { path: PathBuf, source: FenceError },

    #[error("failed to {action} {path}: {source}")]
    Io {
        action: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
}
