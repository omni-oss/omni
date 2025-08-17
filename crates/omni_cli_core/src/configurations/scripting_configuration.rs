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
pub struct ScriptingConfiguration {
    #[serde(default)]
    pub js: JsScriptingConfiguration,
}

#[derive(
    Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema, Copy,
)]
pub struct JsScriptingConfiguration {
    #[serde(default)]
    pub runtime: JsRuntime,
}

impl Default for JsScriptingConfiguration {
    fn default() -> Self {
        Self {
            runtime: JsRuntime::Auto,
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
