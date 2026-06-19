use omni_generator::Action;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GeneratorListResult {
    pub generators: Vec<GeneratorSummary>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GeneratorSummary {
    pub name: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GeneratorInspectParams {
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GeneratorInspectResult {
    pub name: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub inputs: Vec<McpInputSpec>,
    pub targets: Vec<McpTargetSpec>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct McpInputSpec {
    pub name: String,
    pub message: String,
    pub kind: String,
    pub required: bool,
    pub default: Option<Value>,
    pub has_dynamic_default: bool,
    pub options: Vec<McpInputOption>,
    pub condition: Option<McpInputCondition>,
    pub validators: Vec<McpValidator>,
    pub remember: bool,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct McpInputOption {
    pub label: String,
    pub description: Option<String>,
    pub value: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum McpInputCondition {
    AlwaysHidden,
    Expression { expr: String },
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct McpValidator {
    pub condition: String,
    pub error_message: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct McpTargetSpec {
    pub key: String,
    pub default_path: String,
}

fn default_true() -> bool {
    true
}

fn default_empty_obj() -> Value {
    Value::Object(Default::default())
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GeneratorRunParams {
    pub name: String,
    pub output_dir: String,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default = "default_empty_obj")]
    pub input_values: Value,
    #[serde(default = "default_true")]
    pub use_defaults: bool,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default = "default_true")]
    #[schemars(
        description = "When true, the generator's prompted inputs will be saved so they can be reused for future runs."
    )]
    pub save_session: bool,
    #[serde(default)]
    #[schemars(
        description = "When true, the generator will ignore any sessions's saved inputs and targets for current run."
    )]
    pub ignore_session: bool,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GeneratorRunResult {
    pub ok: bool,
    pub session_saved: bool,
    pub actions: Vec<Action>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GeneratorValidateInputParams {
    pub name: String,
    /// JSON object mapping input names to their values.
    #[serde(default = "default_empty_obj")]
    pub input_values: Value,
    /// When `true` (default), inputs with a default value are not flagged as missing.
    #[serde(default = "default_true")]
    pub use_defaults: bool,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GeneratorValidateInputResult {
    pub valid: bool,
    pub errors: Vec<McpInputFieldError>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct McpInputFieldError {
    pub input_name: String,
    pub message: String,
}
