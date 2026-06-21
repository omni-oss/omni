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
    /// Sub-generators invoked by `run-generator` actions, in declaration order.
    pub sub_generators: Vec<McpSubGeneratorRef>,
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
    pub description: Option<String>,
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
    #[serde(default)]
    #[schemars(
        description = "Maximum run-generator nesting depth before the run is aborted. Omit to use the default. Raise it if a generator legitimately nests deeper than the default."
    )]
    pub max_depth: Option<usize>,
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
    pub sub_generators: Vec<McpSubGeneratorValidationResult>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct McpSubGeneratorValidationResult {
    pub generator_name: String,
    /// The `if` expression on the `run-generator` action, if any.
    pub action_condition: Option<String>,
    pub valid: bool,
    pub errors: Vec<McpInputFieldError>,
    pub sub_generators: Vec<McpSubGeneratorValidationResult>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct McpInputFieldError {
    pub input_name: String,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum McpForwardedInputs {
    /// All parent inputs are forwarded into the sub-generator's context.
    All,
    /// No parent inputs are forwarded.
    None,
    /// Only the named parent inputs are forwarded.
    Selected { names: Vec<String> },
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct McpSubGeneratorRef {
    /// The generator name as written in the config.
    pub name: String,
    /// The `if` expression on the `run-generator` action, if any.
    pub action_condition: Option<String>,
    /// Which parent inputs flow into the sub-generator's context automatically.
    pub forwarded_inputs: McpForwardedInputs,
    /// Inputs pre-set with static values in the action config, as a JSON object.
    pub pre_filled_inputs: Value,
    /// Recursive inspect result; absent when name is dynamic or a cycle was detected.
    pub generator: Option<Box<GeneratorInspectResult>>,
}
