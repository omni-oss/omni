//! The error raised when `code_explain` is given a code that does not exist.

use omni_diagnostic::Diagnostic;

omni_codes::prefixes!("code");

/// The code is not in the registry.
///
/// Codes appear in notices and diagnostics; `omni code list` prints them all.
/// Check the spelling, or pick one of the close matches.
#[derive(Debug, thiserror::Error, Diagnostic)]
#[error("unknown code `{code}`")]
#[diagnostic(
    code = "code.unknown",
    audience = user,
    details = unknown_code_details,
    hints = unknown_code_hints,
)]
pub struct UnknownCode {
    pub code: String,
    pub close_matches: Vec<String>,
}

fn unknown_code_details(e: &UnknownCode) -> serde_json::Value {
    serde_json::json!({ "code": e.code, "close_matches": e.close_matches })
}

fn unknown_code_hints(e: &UnknownCode) -> Vec<String> {
    if e.close_matches.is_empty() {
        Vec::new()
    } else {
        vec![format!("did you mean: {}?", e.close_matches.join(", "))]
    }
}

impl UnknownCode {
    /// Wraps a registry miss as a user-facing diagnostic.
    pub fn from_registry(e: omni_codes::UnknownCode) -> Self {
        Self {
            code: e.code,
            close_matches: e
                .close_matches
                .into_iter()
                .map(|c| c.code.to_string())
                .collect(),
        }
    }
}

pub(crate) fn unknown_code(e: omni_codes::UnknownCode) -> UnknownCode {
    UnknownCode::from_registry(e)
}
