use omni_configurations::{ProjectConfiguration, WorkspaceConfiguration};
use omni_generator_configurations::GeneratorConfiguration;
use schemars::schema_for;
use serde::{Deserialize, Serialize};

// ── Kind ─────────────────────────────────────────────────────────────────────

/// Which configuration schema to return.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SchemaKind {
    Workspace,
    Project,
    Generator,
}

// ── Response ──────────────────────────────────────────────────────────────────

/// A JSON Schema document.
#[derive(Debug, Serialize, Deserialize)]
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
    };

    let schema = serde_json::to_value(&schemars_schema)?;
    Ok(ConfigSchemaResponse { schema })
}
