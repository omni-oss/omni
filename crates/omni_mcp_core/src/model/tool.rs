use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

fn default_empty_obj() -> Value {
    Value::Object(Default::default())
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ToolListResult {
    pub tools: Vec<ToolSummary>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ToolSummary {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ToolInspectParams {
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ToolInspectResult {
    pub name: String,
    pub description: Option<String>,
    /// JSON Schema derived from the tool's own `inputs` block (for a pipeline,
    /// from the pipeline's inputs, not its steps').
    pub input_schema: Value,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ToolRunParams {
    pub name: String,
    /// JSON object of arguments, validated against the tool's declared inputs.
    #[serde(default = "default_empty_obj")]
    pub args: Value,
    /// Directory the tool operates in; relative `ctx.sys` paths resolve here.
    /// Defaults to the workspace root.
    #[serde(default)]
    pub working_dir: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ToolRunResult {
    /// The tool's captured return value.
    pub result: Value,
}
