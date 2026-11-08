use std::path::PathBuf;

use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

omni_codes::prefixes!("tool");

#[derive(Debug, thiserror::Error, new, omni_diagnostic::Diagnostic)]
#[error(transparent)]
#[diagnostic(forward)]
pub struct Error(pub(crate) ErrorInner);

impl Error {
    pub fn custom(message: impl Into<String>) -> Self {
        Self(ErrorInner::Custom(eyre::Report::msg(message.into())))
    }

    #[allow(unused)]
    pub fn kind(&self) -> ErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<ErrorInner>> From<T> for Error {
    fn from(inner: T) -> Self {
        Self(inner.into())
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

    /// The named tool is not defined in the workspace.
    #[error("tool '{name}' not found")]
    #[diagnostic(
        code = "tool.not_found",
        hint = "run `omni tool list` to see the available tools"
    )]
    ToolNotFound {
        #[diagnostic(detail)]
        name: String,
    },

    // Constructed by the tool registry when names collide.
    /// Two tool manifests declare the same name.
    #[allow(dead_code)]
    #[error(
        "there is already a tool with the name '{name}', tool names must be unique, config path: {path}"
    )]
    #[diagnostic(
        code = "tool.duplicate_name",
        hint = "rename one of the tools so that every tool name is unique, starting with the one at {path:?}"
    )]
    DuplicateToolName {
        #[diagnostic(detail)]
        name: String,
        #[diagnostic(detail)]
        path: PathBuf,
    },

    /// A tool configuration could not be loaded.
    #[error("failed to load config from '{path}'")]
    #[diagnostic(
        code = "tool.config_load_failed",
        hint = "check the format and readability of the tool configuration at {path:?}"
    )]
    LoadConfig {
        #[diagnostic(detail)]
        path: PathBuf,
        #[source]
        inner: omni_file_data_serde::Error,
    },

    #[error(transparent)]
    #[diagnostic(delegate)]
    ToolDiscovery(#[from] omni_configuration_discovery::error::Error),

    #[error(transparent)]
    #[diagnostic(delegate)]
    FileDiscovery(#[from] omni_discovery::error::Error),

    /// No JavaScript runtime (deno, node or bun) is installed.
    #[error(
        "no JavaScript runtime (deno/node/bun) was found on PATH; install one (deno recommended) to run tools"
    )]
    #[diagnostic(
        code = "tool.no_js_runtime",
        audience = environment,
        hint = "install deno (recommended), node or bun, and make sure it is on PATH"
    )]
    NoJsRuntime,

    /// The JavaScript runtime selected for a tool is not installed.
    #[error(
        "the '{runtime}' runtime was selected for this tool but was not found on PATH; install it or change the tool's `runtime`"
    )]
    #[diagnostic(
        code = "tool.runtime_not_found",
        audience = environment,
        hint = "install `{runtime}` and make sure it is on PATH, or change the tool's `runtime`"
    )]
    RuntimeNotFound {
        #[diagnostic(detail)]
        runtime: String,
    },

    // Reserved for a future non-JS backend (the `type` discriminant reserves
    // `command`): a manifest may name a backend this build cannot execute.
    /// A tool names a backend this build cannot execute.
    #[allow(dead_code)]
    #[error("the tool backend '{backend}' is not yet supported")]
    #[diagnostic(
        code = "tool.unsupported_backend",
        hint = "set the tool's `type` to `js` or `pipeline`"
    )]
    UnsupportedBackend {
        #[diagnostic(detail)]
        backend: String,
    },

    /// The selected runtime and platform cannot enforce the tool's capability policy.
    #[error("cannot enforce the capability policy for this tool: {message}")]
    #[diagnostic(code = "tool.capability_enforcement_failed", audience = environment, hint = "use a runtime and platform that support the required capability restrictions; do not disable required protections")]
    CapabilityEnforcement { message: String },

    /// Pipeline tools nest deeper than the allowed maximum.
    #[error(
        "pipeline nesting exceeded the maximum depth of {max}; this usually indicates a cycle between pipeline tools"
    )]
    #[diagnostic(
        code = "tool.max_depth_exceeded",
        hint = "check the pipeline for a tool that runs itself, directly or through other pipeline tools"
    )]
    MaxDepthExceeded {
        #[diagnostic(detail)]
        max: usize,
    },

    /// A pipeline step's `if` condition did not evaluate to a boolean.
    #[error(
        "the `if` condition for pipeline step '{step}' ({expr}) evaluated to '{result}', expected 'true' or 'false'"
    )]
    #[diagnostic(
        code = "tool.invalid_if_condition",
        hint = "make the `if` condition of pipeline step `{step}` evaluate to `true` or `false`"
    )]
    InvalidIfCondition {
        #[diagnostic(detail)]
        step: String,
        expr: String,
        result: String,
    },

    /// A pipeline step's input or `if` condition failed to render.
    #[error("failed to render a pipeline step input or condition")]
    #[diagnostic(
        code = "tool.template_failed",
        hint = "check the template syntax of the pipeline step inputs and `if` conditions"
    )]
    Tera(#[from] omni_tera::Error),

    #[error(transparent)]
    #[diagnostic(delegate)]
    Runner(#[from] bridge_rpc_runner::BridgeRunnerError),

    #[error(transparent)]
    #[diagnostic(skip)]
    SerdeJson(#[from] serde_json::Error),
}
