use std::path::PathBuf;

use crate::{
    ActionConfiguration, Generator, OmniPath,
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
    #[serde(default)]
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
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use maps::UnorderedMap;
    use omni_input_schema::{Input, InputKind, ValidationConfig, validate};
    use serde_json::json;
    use value_bag::ValueBag;

    use super::GeneratorConfiguration;
    use crate::Generator;

    #[test]
    fn deserializes_data_typed_inputs_from_json() {
        let config: GeneratorConfiguration = serde_json::from_value(json!({
            "name": "my-gen",
            "inputs": [
                {"type": "boolean", "name": "dry_run", "message": "Dry run?", "default": false},
                {"type": "string",  "name": "project_name", "message": "Project name"},
                {"type": "integer", "name": "port", "message": "Port number", "default": 3000},
                {"type": "float",   "name": "ratio", "message": "Ratio"}
            ],
            "actions": []
        }))
        .expect("should parse");
        assert_eq!(config.inputs.len(), 4);
        assert_eq!(config.inputs[0].base().name, "dry_run");
        assert_eq!(config.inputs[0].kind(), InputKind::Boolean);
        assert_eq!(config.inputs[1].kind(), InputKind::String);
        assert_eq!(config.inputs[2].kind(), InputKind::Integer);
        assert_eq!(config.inputs[3].kind(), InputKind::Float);
    }

    #[test]
    fn validate_rejects_object_input_for_generator_profile() {
        // Object is not in Generator::SUPPORTED — it deserializes (serde is
        // monomorphic) but validate() must reject it.
        let object_input: Input<Generator> = serde_json::from_value(json!({
            "type": "object",
            "name": "opts",
            "message": "Options",
            "fields": []
        }))
        .expect("should deserialize");

        let values = UnorderedMap::from_iter([(
            "opts".to_string(),
            ValueBag::from_serde1(&json!({})).to_owned(),
        )]);
        let ctx = omni_tera::Context::new();
        let result = validate(
            &[object_input],
            &values,
            &ctx,
            &ValidationConfig::default(),
        );
        assert!(
            result.is_err(),
            "validate should reject Object kind for Generator profile"
        );
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("opts"), "error should name the input: {msg}");
    }
}
