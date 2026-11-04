use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CodeExplainParams {
    /// An error or notice code, for example `meta.filter_failed`.
    pub code: String,
}
