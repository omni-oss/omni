use std::path::PathBuf;

use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

#[derive(Debug, thiserror::Error, new)]
#[error(transparent)]
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

#[derive(Debug, thiserror::Error, EnumDiscriminants, new)]
#[strum_discriminants(vis(pub), name(ErrorKind))]
pub(crate) enum ErrorInner {
    #[error(transparent)]
    Custom(#[from] eyre::Report),

    #[error("tool '{name}' not found")]
    ToolNotFound { name: String },

    // Constructed by the tool registry when names collide.
    #[allow(dead_code)]
    #[error(
        "there is already a tool with the name '{name}', tool names must be unique, config path: {path}"
    )]
    DuplicateToolName { name: String, path: PathBuf },

    #[error("failed to load config from '{path}'")]
    LoadConfig {
        path: PathBuf,
        #[source]
        inner: omni_file_data_serde::Error,
    },

    #[error(transparent)]
    ToolDiscovery(#[from] omni_configuration_discovery::error::Error),

    #[error(transparent)]
    FileDiscovery(#[from] omni_discovery::error::Error),

    #[error(
        "no JavaScript runtime (deno/node/bun) was found on PATH; install one (deno recommended) to run tools"
    )]
    NoJsRuntime,

    #[error(
        "the '{runtime}' runtime was selected for this tool but was not found on PATH; install it or change the tool's `runtime`"
    )]
    RuntimeNotFound { runtime: String },

    // Reserved for a future non-JS backend (the `type` discriminant reserves
    // `command`): a manifest may name a backend this build cannot execute.
    #[allow(dead_code)]
    #[error("the tool backend '{backend}' is not yet supported")]
    UnsupportedBackend { backend: String },

    #[error("cannot enforce the capability policy for this tool: {message}")]
    CapabilityEnforcement { message: String },

    #[error(
        "pipeline nesting exceeded the maximum depth of {max}; this usually indicates a cycle between pipeline tools"
    )]
    MaxDepthExceeded { max: usize },

    #[error(
        "the `if` condition for pipeline step '{step}' ({expr}) evaluated to '{result}', expected 'true' or 'false'"
    )]
    InvalidIfCondition {
        step: String,
        expr: String,
        result: String,
    },

    #[error("failed to render a pipeline step input or condition")]
    Tera(#[from] omni_tera::Error),

    #[error(transparent)]
    Runner(#[from] bridge_rpc_runner::BridgeRunnerError),

    #[error(transparent)]
    SerdeJson(#[from] serde_json::Error),
}
