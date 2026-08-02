use std::path::PathBuf;

use crate::{
    ActionConfiguration, CapabilityPolicyConfig, Generator, OmniPath,
    validators::{validate_umap_serde_json, validate_umap_target_path},
};
use garde::Validate;
use maps::UnorderedMap;
use omni_input_schema::Input;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate,
)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields)]
pub struct GeneratorConfiguration {
    /// Absolute path to the configuration file where this is serialized from
    #[serde(default)]
    #[serde(skip)]
    pub config_path: PathBuf,

    /// Scope identifier. Once set, only generators with the same scope id will be able to call and be called by this generator.
    #[serde(default)]
    #[serde(skip)]
    pub scope_id: Option<String>,

    /// Whether this generator can be invoked by the user directly. If false, it can only be invoked by other generators.
    #[serde(default = "default_true")]
    pub user_invocable: bool,

    /// Unique name of the generator
    pub name: String,

    /// Display name of the generator, if not provided, the name will be used
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,

    /// Description of the generator
    pub description: Option<String>,

    /// Prompts to ask the user
    #[serde(default)]
    #[serde(alias = "prompts")]
    pub inputs: Vec<Input<Generator>>,

    /// Actions to perform
    #[serde(default)]
    pub actions: Vec<ActionConfiguration>,

    /// Variables to use in the generator, these are evaluated after the prompts
    ///
    /// The variables are available in the templates as `{{ vars.var_name }}`
    ///
    /// Available context variables:
    /// - `inputss`: The values of the prompts
    #[serde(default)]
    #[serde(deserialize_with = "validate_umap_serde_json")]
    pub vars: UnorderedMap<String, serde_json::Value>,

    /// Target directories to place the generated files
    /// Target directories to add the file(s) to. If it does not exist, it will be created.
    #[serde(deserialize_with = "validate_umap_target_path")]
    #[serde(default)]
    pub targets: UnorderedMap<String, OmniPath>,

    /// Capability policy governing what this generator's scripts may do
    /// (filesystem / process / network access).
    ///
    /// An object of `{ rules, strictness }`:
    ///
    /// * `rules` cascade by concatenation over the surrounding levels
    ///   (workspace ⟺ generator ⟺ action); a matching `deny` always wins, so a
    ///   more specific level can only ever *narrow* authority. Only the domains
    ///   the generator subsystem supports (`fs.read`, `fs.write`, `process`,
    ///   `net`) are accepted — an unsupported domain is rejected when the config
    ///   is loaded.
    /// * `strictness` (`warn` default, or `require-floor`) controls how a
    ///   *floor gap* is treated: a governed domain that, on the resolved
    ///   runtime/platform, rests only on a bypassable in-process mechanism with
    ///   no un-bypassable runtime-flag or OS-sandbox floor. `require-floor`
    ///   refuses to run unless every governed domain has such a floor.
    ///   Strictness combines most-severe across the workspace, generator, and
    ///   action levels.
    #[serde(default)]
    pub capabilities: CapabilityPolicyConfig<Generator>,
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use omni_input_schema::InputKind;
    use serde_json::json;

    use super::GeneratorConfiguration;

    #[test]
    fn capabilities_strictness_defaults_to_warn_and_parses_require_floor() {
        use crate::CapabilitiesStrictness;

        // Absent → the default `Warn` stance.
        let default_cfg: GeneratorConfiguration =
            serde_json::from_value(json!({ "name": "g", "actions": [] }))
                .expect("parses without the field");
        assert_eq!(
            default_cfg.capabilities.strictness,
            CapabilitiesStrictness::Warn
        );

        // Present, in the kebab-case wire form, nested under `capabilities`.
        let strict_cfg: GeneratorConfiguration =
            serde_json::from_value(json!({
                "name": "g",
                "actions": [],
                "capabilities": { "strictness": "require-floor" }
            }))
            .expect("parses require-floor");
        assert_eq!(
            strict_cfg.capabilities.strictness,
            CapabilitiesStrictness::RequireFloor
        );

        // An unknown stance is rejected rather than silently ignored.
        let bad: Result<GeneratorConfiguration, _> =
            serde_json::from_value(json!({
                "name": "g",
                "actions": [],
                "capabilities": { "strictness": "yolo" }
            }));
        assert!(bad.is_err(), "an unknown strictness value must be rejected");
    }

    #[test]
    fn deserializes_data_typed_inputs_from_json() {
        let config: GeneratorConfiguration = serde_json::from_value(json!({
            "name": "my-gen",
            "inputs": [
                {"type": "boolean", "name": "dry_run", "message": "Dry run?", "default": false},
                {"type": "string",  "name": "project_name", "message": "Project name"},
                {"type": "integer", "name": "port", "message": "Port number", "default": 3000},
                {"type": "float",   "name": "ratio", "message": "Ratio"},
                {
                    "type": "object",
                    "name": "opts",
                    "message": "Options",
                    "fields": []
                }
            ],
            "actions": []
        }))
        .expect("should parse");
        assert_eq!(config.inputs.len(), 5);
        assert_eq!(config.inputs[0].base().name, "dry_run");
        assert_eq!(config.inputs[0].kind(), InputKind::Boolean);
        assert_eq!(config.inputs[1].kind(), InputKind::String);
        assert_eq!(config.inputs[2].kind(), InputKind::Integer);
        assert_eq!(config.inputs[3].kind(), InputKind::Float);
        assert_eq!(config.inputs[4].kind(), InputKind::Object);
    }
}
