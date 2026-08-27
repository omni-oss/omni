use std::borrow::Borrow;

use lazy_regex::{Lazy, Regex, regex};
use serde_validate::{StaticValidator, declare_static_validator};

#[derive(Debug, Clone, Copy, Default)]
struct ProjectionIdValidator;

static PROJECTION_ID_SEGMENT_REGEX: &Lazy<Regex> =
    regex!(r"^@?[A-Za-z0-9](?:[A-Za-z0-9._-]*[A-Za-z0-9])?$");

fn is_windows_reserved_name(segment: &str) -> bool {
    let stem = segment.split('.').next().unwrap_or(segment);
    let stem = stem.to_ascii_uppercase();

    match stem.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" => true,
        _ => {
            let is_numbered = |prefix: &str| {
                stem.strip_prefix(prefix).is_some_and(|n| {
                    n.len() == 1 && matches!(n.as_bytes()[0], b'1'..=b'9')
                })
            };
            is_numbered("COM") || is_numbered("LPT")
        }
    }
}

impl<T: Borrow<String>> StaticValidator<T> for ProjectionIdValidator {
    fn validate_static(value: &T) -> Result<(), String> {
        let id = value.borrow();

        if id.is_empty() {
            return Err("projection id must not be empty".to_string());
        }
        if id.contains('\\') {
            return Err(format!(
                "projection id must use `/` as the only path separator: {id}"
            ));
        }
        if id.starts_with('/') {
            return Err(format!("projection id must be relative: {id}"));
        }

        for segment in id.split('/') {
            if segment.is_empty() {
                return Err(format!(
                    "projection id must not contain empty segments: {id}"
                ));
            }
            if segment == "." || segment == ".." {
                return Err(format!(
                    "projection id must not contain `.` or `..` segments: {id}"
                ));
            }
            if !PROJECTION_ID_SEGMENT_REGEX.is_match(segment) {
                return Err(format!(
                    "invalid projection id segment `{segment}` in: {id}"
                ));
            }
            if is_windows_reserved_name(segment) {
                return Err(format!(
                    "projection id segment `{segment}` is a reserved device name: {id}"
                ));
            }
        }

        Ok(())
    }
}

declare_static_validator!(
    ProjectionIdValidator,
    String,
    validate_projection_id,
    option_validate_projection_id,
);
