//! The error raised when `explain` is given a code that does not exist.

use omni_diagnostic::Diagnostic;

omni_codes::prefixes!("explain");

/// The code is not in the registry.
///
/// Codes are listed by `omni explain --list`-style tooling and appear in
/// notices and diagnostics. Check the spelling, or pick one of the close
/// matches.
#[derive(Debug, thiserror::Error, Diagnostic)]
#[error("unknown code `{code}`")]
#[diagnostic(
    code = "explain.unknown_code",
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

pub(crate) fn unknown_code(e: omni_codes::UnknownCode) -> UnknownCode {
    UnknownCode {
        code: e.code,
        close_matches: e
            .close_matches
            .into_iter()
            .map(|c| c.code.to_string())
            .collect(),
    }
}

pub use UnknownCode as ExplainUnknownCode;
