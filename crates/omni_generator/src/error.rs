use std::path::PathBuf;

use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::action_handlers::utils::ResolveOutputPathError;

#[derive(Debug, thiserror::Error, new, omni_diagnostic::Diagnostic)]
#[error(transparent)]
#[diagnostic(forward)]
pub struct Error(pub(crate) ErrorInner);

impl Error {
    pub fn custom<T: Into<String>>(msg: T) -> Self {
        Self(ErrorInner::Custom(eyre::Report::msg(msg.into())))
    }
}

impl Error {
    #[allow(unused)]
    pub fn kind(&self) -> ErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<ErrorInner>> From<T> for Error {
    fn from(inner: T) -> Self {
        let inner = inner.into();

        Self(inner)
    }
}

#[derive(
    Debug, thiserror::Error, EnumDiscriminants, new, omni_diagnostic::Diagnostic,
)]
#[strum_discriminants(vis(pub), name(ErrorKind))]
pub(crate) enum ErrorInner {
    #[error(transparent)]
    #[diagnostic(delegate)]
    Custom(#[from] eyre::Report),

    /// The named generator is not defined in the workspace.
    #[error("generator '{name}' not found")]
    #[diagnostic(
        code = "generator.not_found",
        hint = "run `omni generator list` to see the available generators"
    )]
    GeneratorNotFound {
        #[diagnostic(detail)]
        name: String,
    },

    /// The generator config file could not be loaded.
    #[error("failed to load config from '{path}'")]
    #[diagnostic(
        code = "generator.config_load_failed",
        hint = "fix the syntax or schema errors in {path:?}"
    )]
    LoadConfig {
        #[diagnostic(detail)]
        path: PathBuf,
        #[source]
        inner: omni_file_data_serde::Error,
    },

    #[error(transparent)]
    #[diagnostic(skip)]
    Tera(#[from] omni_tera::Error),

    #[error(transparent)]
    #[diagnostic(delegate)]
    Resolve(#[from] omni_command_config::ResolveError),

    /// An expression did not evaluate to a boolean.
    // The expression and its result are deliberately not in the message: a
    // rendered condition may contain a secret input.
    #[error("expression in '{expr_name}' did not evaluate to a boolean")]
    #[diagnostic(
        code = "generator.invalid_boolean_result",
        hint = "make the expression in `{expr_name}` evaluate to `true` or `false`"
    )]
    InvalidBooleanResult {
        #[new(into)]
        result: String,
        #[new(into)]
        expr: String,
        #[new(into)]
        #[diagnostic(detail)]
        expr_name: String,
    },

    #[error(transparent)]
    #[diagnostic(delegate)]
    Collect(#[from] omni_input_provider::error::Error),

    #[error(transparent)]
    #[diagnostic(delegate)]
    GeneratorDiscovery(#[from] omni_configuration_discovery::error::Error),

    #[error(transparent)]
    #[diagnostic(delegate)]
    FileDiscovery(#[from] omni_discovery::error::Error),

    /// Two generator configs declare the same generator name.
    #[error(
        "there is already a generator with the name '{name}', generator names must be unique, config path: {path}"
    )]
    #[diagnostic(
        code = "generator.duplicate_name",
        hint = "rename one of the generators named `{name}` so that every generator name is unique, starting with the one at {path:?}"
    )]
    DuplicateGeneratorName {
        #[diagnostic(detail)]
        name: String,
        #[diagnostic(detail)]
        path: PathBuf,
    },

    /// A file could not be written.
    #[error("failed to write to path '{path}', error: {error}")]
    #[diagnostic(
        code = "generator.write_failed",
        audience = environment,
        hint = "check that {path:?} is writable and that its parent directory exists"
    )]
    FailedToWriteFile {
        #[new(into)]
        #[diagnostic(detail)]
        path: PathBuf,
        #[source]
        #[new(into)]
        error: std::io::Error,
    },

    /// A file could not be read.
    #[error("failed to read from path '{path}', error: {error}")]
    #[diagnostic(
        code = "generator.read_failed",
        audience = environment,
        hint = "check that {path:?} is readable"
    )]
    FailedToReadFile {
        #[new(into)]
        #[diagnostic(detail)]
        path: PathBuf,

        #[source]
        #[new(into)]
        error: std::io::Error,
    },

    /// An output path exists but is a file, not a directory.
    #[error("path exists but it is not a directory: '{path}")]
    #[diagnostic(
        code = "generator.path_not_dir",
        hint = "remove or rename {path:?}, or choose a different output directory"
    )]
    PathExistsButNotDir {
        #[new(into)]
        #[diagnostic(detail)]
        path: PathBuf,
    },

    #[error(transparent)]
    #[diagnostic(skip)]
    GenericIo(#[from] std::io::Error),

    #[error(transparent)]
    #[diagnostic(delegate)]
    ResolveOutputPath(#[from] ResolveOutputPathError),

    #[error(transparent)]
    #[diagnostic(skip)]
    FromUtf8(#[from] std::string::FromUtf8Error),

    #[error(transparent)]
    #[diagnostic(skip)]
    SerdeJson(#[from] serde_json::Error),

    #[error(transparent)]
    #[diagnostic(skip)]
    Regex(#[from] regex::Error),

    /// A file that was expected does not exist.
    #[error("file not found: {path}")]
    #[diagnostic(
        code = "generator.file_not_found",
        hint = "check that {path:?} exists"
    )]
    FileNotFound {
        #[diagnostic(detail)]
        path: PathBuf,
        #[source]
        error: std::io::Error,
    },

    #[error(transparent)]
    #[diagnostic(delegate)]
    ChildProcess(#[from] omni_process::ChildProcessError),

    /// A generator command exited with a non-zero status.
    // Only the program is named: the rest of the command line may hold
    // rendered secrets.
    #[error(
        "command '{program}' failed with exit code {exit_code}",
        program = program_of(command)
    )]
    #[diagnostic(
        code = "generator.command_failed",
        hint = "check the output above for why the command failed, then fix it or the generator's command"
    )]
    CommandFailed {
        #[new(into)]
        command: String,
        #[diagnostic(detail)]
        exit_code: u32,
    },

    #[error(transparent)]
    #[diagnostic(delegate)]
    Expansion(#[from] env::ExpansionError),

    /// The generator is not marked as invocable by the user.
    #[error("generator '{name}' is not invocable by the user")]
    #[diagnostic(
        code = "generator.not_invocable",
        hint = "run a generator that can be invoked by the user; see `omni generator list`"
    )]
    GeneratorNotInvocable {
        #[new(into)]
        #[diagnostic(detail)]
        name: String,
    },

    /// A generator call would run the generator again from within itself.
    #[error(
        "generator '{name}' call will recurse into itself, config path: {config_path}"
    )]
    #[diagnostic(
        code = "generator.recursion",
        hint = "remove the generator call that makes `{name}` run itself, in {config_path:?}"
    )]
    GeneratorRecursion {
        #[new(into)]
        #[diagnostic(detail)]
        name: String,
        #[new(into)]
        #[diagnostic(detail)]
        config_path: PathBuf,
    },

    /// Generators nest deeper than the allowed maximum.
    #[error(
        "generator '{name}' exceeded the maximum nesting depth of {limit}; if this nesting is intentional, raise max_depth"
    )]
    #[diagnostic(
        code = "generator.max_depth_exceeded",
        hint = "check for a generator that calls itself, directly or through other generators, or raise `max_depth` if the nesting is intended"
    )]
    MaxGeneratorDepthExceeded {
        #[new(into)]
        #[diagnostic(detail)]
        name: String,
        #[diagnostic(detail)]
        limit: usize,
    },

    /// A generator's capability policy is invalid.
    #[error(
        "generator '{name}' has an invalid capability policy (config path: {config_path})"
    )]
    #[diagnostic(
        code = "generator.invalid_capabilities",
        hint = "fix the capability policy of generator `{name}` in {config_path:?}"
    )]
    InvalidCapabilities {
        #[new(into)]
        #[diagnostic(detail)]
        name: String,
        #[new(into)]
        #[diagnostic(detail)]
        config_path: PathBuf,
        #[source]
        source: omni_capabilities::Error,
    },

    /// The workspace-level capability policy is invalid.
    #[error("the workspace-level capability policy is invalid")]
    #[diagnostic(
        code = "generator.invalid_workspace_capabilities",
        hint = "fix the capability policy in the workspace configuration"
    )]
    InvalidWorkspaceCapabilities {
        #[source]
        source: omni_capabilities::Error,
    },
}

/// The program of a command line, without its arguments.
fn program_of(command: &str) -> &str {
    command.split_whitespace().next().unwrap_or("")
}
