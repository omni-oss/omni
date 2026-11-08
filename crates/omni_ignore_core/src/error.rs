use std::path::{Path, PathBuf};

omni_codes::prefixes!("ignore");

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

impl FenceError {
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::DuplicateBegin(_) => "multiple_begin",
            Self::DuplicateEnd(_) => "multiple_end",
            Self::UnclosedBegin => "missing_end",
            Self::UnopenedEnd => "missing_begin",
            Self::CrossedFence => "end_before_begin",
        }
    }
}

/// Errors raised while building patterns or patching ignore files.
#[derive(thiserror::Error, Debug, omni_diagnostic::Diagnostic)]
pub enum IgnoreError {
    /// An ignore pattern is an empty string.
    #[error("ignore pattern is empty")]
    #[diagnostic(
        code = "ignore.empty_pattern",
        hint = "remove the empty ignore pattern from your workspace or project configuration"
    )]
    EmptyPattern,

    /// An ignore pattern spans more than one line.
    #[error(
        "ignore pattern must be a single line but contains a line break: {0:?}"
    )]
    #[diagnostic(
        code = "ignore.multi_line_pattern",
        hint = "put each ignore pattern on its own line, without line breaks"
    )]
    MultiLinePattern(String),

    /// An ignore pattern is the same line as a managed fence marker.
    #[error("ignore pattern collides with a managed fence marker: {0:?}")]
    #[diagnostic(
        code = "ignore.pattern_collides_with_fence",
        hint = "change the ignore pattern so it is not a `# @@omni-managed` marker line"
    )]
    FenceInPattern(String),

    /// An ignore file has a malformed managed block, so it cannot be updated safely.
    #[error("{path}: {source}")]
    #[diagnostic(
        code = "ignore.invalid_fence",
        hint = "remove the extra or misplaced `@@omni-managed` marker lines from {path:?}, then run `omni ignore sync` again"
    )]
    MalformedFence {
        path: PathBuf,
        #[diagnostic(detail)]
        kind: &'static str,
        source: FenceError,
    },

    /// A file the ignore sync reads or writes could not be accessed.
    #[error("failed to {action} {path}: {source}")]
    #[diagnostic(
        code = "ignore.io_failed",
        audience = environment,
        hint = "check that {path:?} exists and that omni can {action} it"
    )]
    Io {
        #[diagnostic(detail)]
        action: &'static str,
        #[diagnostic(detail)]
        path: PathBuf,
        source: std::io::Error,
    },
}

impl IgnoreError {
    pub(crate) fn malformed_fence(path: &Path, source: FenceError) -> Self {
        Self::MalformedFence {
            path: path.to_path_buf(),
            kind: source.kind(),
            source,
        }
    }
}
