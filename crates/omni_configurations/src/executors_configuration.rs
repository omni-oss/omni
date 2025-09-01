use garde::Validate;
use js_runtime::impls::DelegatingJsRuntimeOption;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Deserialize,
    Serialize,
    JsonSchema,
    Copy,
    Default,
    Validate,
)]
#[garde(allow_unvalidated)]
pub struct ExecutorsConfiguration {
    #[serde(default)]
    pub javascript: JsScriptingConfiguration,

    #[serde(default)]
    pub task: TaskExecutorConfiguration,
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Deserialize,
    Serialize,
    JsonSchema,
    Copy,
    Default,
)]
pub struct JsScriptingConfiguration {
    #[serde(default)]
    pub runtime: JsRuntime,
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Deserialize,
    Serialize,
    JsonSchema,
    Copy,
    Default,
)]
#[serde(rename_all = "kebab-case")]
pub enum JsRuntime {
    Deno,
    Node,
    Bun,
    #[default]
    Auto,
}

impl From<JsRuntime> for DelegatingJsRuntimeOption {
    fn from(val: JsRuntime) -> Self {
        match val {
            JsRuntime::Deno => DelegatingJsRuntimeOption::Deno,
            JsRuntime::Node => DelegatingJsRuntimeOption::Node,
            JsRuntime::Bun => DelegatingJsRuntimeOption::Bun,
            JsRuntime::Auto => DelegatingJsRuntimeOption::Auto,
        }
    }
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Deserialize,
    Serialize,
    JsonSchema,
    Copy,
    Default,
)]
pub struct TaskExecutorConfiguration {
    #[serde(default)]
    shell: Shell,
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Deserialize,
    Serialize,
    JsonSchema,
    Copy,
    Default,
)]
#[serde(rename_all = "kebab-case")]
pub enum Shell {
    Sh,
    Bash,
    Zsh,
    Pwsh,
    #[default]
    Auto,
}
