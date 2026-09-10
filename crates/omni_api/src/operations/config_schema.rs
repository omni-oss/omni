use omni_configurations::{
    OwnedProjectionConfiguration, ProjectConfiguration, WorkspaceConfiguration,
};
use omni_generator_configurations::GeneratorConfiguration;
use omni_tool_configurations::ToolConfiguration;
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};

// ── Kind ─────────────────────────────────────────────────────────────────────

/// Which configuration schema to return.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum SchemaKind {
    Workspace,
    Project,
    Generator,
    Tool,
    Projection,
}

// ── Response ──────────────────────────────────────────────────────────────────

/// A JSON Schema document.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ConfigSchemaResponse {
    pub schema: serde_json::Value,
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// Return a JSON Schema for the requested configuration kind.
///
/// This is a pure, synchronous operation — no workspace loading required.
///
/// # Examples
///
/// ```rust
/// use omni_api::{handle_config_schema, SchemaKind};
///
/// let resp = handle_config_schema(SchemaKind::Workspace).expect("schema generation");
/// assert!(resp.schema.is_object());
/// ```
pub fn handle_config_schema(
    kind: SchemaKind,
) -> eyre::Result<ConfigSchemaResponse> {
    let schemars_schema = match kind {
        SchemaKind::Workspace => schema_for!(WorkspaceConfiguration),
        SchemaKind::Project => schema_for!(ProjectConfiguration),
        SchemaKind::Generator => schema_for!(GeneratorConfiguration),
        SchemaKind::Tool => schema_for!(ToolConfiguration),
        SchemaKind::Projection => schema_for!(OwnedProjectionConfiguration),
    };

    let schema = serde_json::to_value(&schemars_schema)?;
    Ok(ConfigSchemaResponse { schema })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_schema_is_a_json_object_with_the_tool_shape() {
        let resp = handle_config_schema(SchemaKind::Tool)
            .expect("tool schema generation");
        assert!(resp.schema.is_object(), "schema must be a JSON object");

        // The tool manifest's shared fields and its `type`-tagged backend
        // discriminant must be present in the generated schema.
        let text = serde_json::to_string(&resp.schema).unwrap();
        assert!(text.contains("\"name\""), "{text}");
        assert!(text.contains("\"inputs\""), "{text}");
        assert!(text.contains("\"capabilities\""), "{text}");
        assert!(text.contains("\"js\""), "{text}");
        assert!(text.contains("\"pipeline\""), "{text}");
    }

    #[test]
    fn projection_schema_is_the_owned_manifest_shape() {
        let resp = handle_config_schema(SchemaKind::Projection)
            .expect("projection schema generation");
        assert!(resp.schema.is_object(), "schema must be a JSON object");

        // The owned `projection.omni` manifest is a `oneOf` of a leaf shape
        // (a `routes` list of strategy-tagged projections) and a meta shape
        // (a `sources` list).
        let arms = resp
            .schema
            .get("oneOf")
            .and_then(|v| v.as_array())
            .expect("manifest schema is a oneOf");
        assert_eq!(arms.len(), 2, "one leaf arm and one meta arm");

        let text = serde_json::to_string(&resp.schema).unwrap();
        assert!(text.contains("\"routes\""), "{text}");
        assert!(text.contains("\"sources\""), "{text}");
        assert!(text.contains("\"strategy\""), "{text}");
    }

    #[test]
    fn every_schema_kind_generates() {
        for kind in [
            SchemaKind::Workspace,
            SchemaKind::Project,
            SchemaKind::Generator,
            SchemaKind::Tool,
            SchemaKind::Projection,
        ] {
            let resp = handle_config_schema(kind).expect("schema generation");
            assert!(resp.schema.is_object());
        }
    }
}
