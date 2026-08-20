use std::path::PathBuf;

use garde::Validate;
use omni_input_schema::InputSchema;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{CapabilityPolicyConfig, Tool, ToolBackend};

/// A discovered tool manifest (`tool.omni.yaml`).
///
/// The `name`, `description`, `inputs`, and `capabilities` fields are shared by
/// every backend; the backend-specific fields are flattened in from
/// [`ToolBackend`] behind the required `type` discriminant.
#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate,
)]
#[garde(allow_unvalidated)]
pub struct ToolConfiguration {
    /// Absolute path to the manifest file this was deserialized from.
    #[serde(default, skip)]
    pub config_path: PathBuf,

    /// Unique name of the tool.
    pub name: String,

    /// Human-readable description of what the tool does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Typed, validated, non-interactive inputs (the data-only input model).
    #[serde(default)]
    pub inputs: Vec<InputSchema>,

    /// Capability policy governing what this tool may do (filesystem / process
    /// / network / environment access). Cascades under the workspace-level
    /// tool policy; a matching `deny` always wins.
    #[serde(default)]
    pub capabilities: CapabilityPolicyConfig<Tool>,

    /// The execution backend, selected by the required `type` field.
    #[serde(flatten)]
    pub backend: ToolBackend,
}

#[cfg(test)]
mod tests {
    use omni_capabilities::CapabilitiesStrictness;
    use serde_json::json;

    use super::*;
    use crate::{PipelineOutput, ToolBackend, ToolJsRuntime};

    #[test]
    fn js_tool_round_trips_with_default_runtime() {
        let cfg: ToolConfiguration = serde_json::from_value(json!({
            "type": "js",
            "name": "summarize",
            "description": "Summarize results",
            "entrypoint": "./dist/index.mjs",
            "inputs": [
                { "type": "string", "name": "dir" },
                { "type": "string", "name": "format", "default": "md", "allowed": ["md", "json"] }
            ]
        }))
        .expect("parses js tool");

        assert_eq!(cfg.name, "summarize");
        assert_eq!(cfg.inputs.len(), 2);
        assert_eq!(cfg.capabilities.strictness, CapabilitiesStrictness::Warn);
        match &cfg.backend {
            ToolBackend::Js {
                runtime,
                entrypoint,
            } => {
                assert_eq!(*runtime, ToolJsRuntime::Deno);
                assert_eq!(entrypoint, "./dist/index.mjs");
            }
            ToolBackend::Pipeline { .. } => panic!("expected js backend"),
        }

        let reparsed: ToolConfiguration =
            serde_json::from_value(serde_json::to_value(&cfg).unwrap())
                .unwrap();
        assert_eq!(cfg, reparsed);
    }

    #[test]
    fn pipeline_tool_round_trips_with_default_output() {
        let cfg: ToolConfiguration = serde_json::from_value(json!({
            "type": "pipeline",
            "name": "fetch-and-summarize",
            "steps": [
                { "name": "fetch", "tool": "fetch-data", "inputs": { "dir": { "from": "inputs.dir" } } },
                { "name": "report", "tool": "summarize", "inputs": { "files": { "from": "steps.fetch.output.files" } } }
            ]
        }))
        .expect("parses pipeline tool");

        match &cfg.backend {
            ToolBackend::Pipeline { steps, output } => {
                assert_eq!(*output, PipelineOutput::Last);
                assert_eq!(steps.len(), 2);
            }
            ToolBackend::Js { .. } => panic!("expected pipeline backend"),
        }

        let reparsed: ToolConfiguration =
            serde_json::from_value(serde_json::to_value(&cfg).unwrap())
                .unwrap();
        assert_eq!(cfg, reparsed);
    }

    #[test]
    fn require_floor_strictness_parses() {
        let cfg: ToolConfiguration = serde_json::from_value(json!({
            "type": "js",
            "name": "t",
            "entrypoint": "./x.mjs",
            "capabilities": { "strictness": "require-floor" }
        }))
        .expect("parses require-floor");
        assert_eq!(
            cfg.capabilities.strictness,
            CapabilitiesStrictness::RequireFloor
        );
    }
}
