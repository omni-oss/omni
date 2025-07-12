use js_runtime::impls::DelegatingJsRuntimeOption;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(
    Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema, Copy,
)]
pub struct ScriptingConfiguration {
    #[serde(default)]
    pub js_runtime: JsRuntime,
}

impl Default for ScriptingConfiguration {
    fn default() -> Self {
        Self {
            js_runtime: JsRuntime::Auto,
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
