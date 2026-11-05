use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CodeExplainParams {
    /// An error or notice code, for example `meta.filter_failed`.
    pub code: String,
}

/// Which kind of code to list.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CodeListKind {
    /// Codes of fatal errors.
    Error,
    /// Codes of notices (non-fatal messages).
    Notice,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct CodeListParams {
    /// Only list codes of this kind. Omit for every code.
    #[serde(default)]
    pub kind: Option<CodeListKind>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CodeListResult {
    /// Registry entries, sorted by code, each with the same fields as
    /// `code_explain`.
    pub codes: Vec<serde_json::Value>,
}
