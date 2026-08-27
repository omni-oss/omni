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

#[cfg(test)]
mod tests {
    use serde_validate::StaticValidator;

    use super::{ProjectionIdValidator, is_windows_reserved_name};

    fn validate(id: &str) -> Result<(), String> {
        ProjectionIdValidator::validate_static(&id.to_string())
    }

    #[test]
    fn accepts_simple_and_scoped_ids() {
        for id in [
            "a",
            "Z",
            "0",
            "team-ai-skills",
            "shared_scripts",
            "a.b.c",
            "a_b-c.d",
            "@myorg",
            "@myorg/pkg",
        ] {
            validate(id)
                .unwrap_or_else(|e| panic!("`{id}` should be valid: {e}"));
        }
    }

    #[test]
    fn accepts_multi_segment_paths() {
        for id in ["a/b/c", "@scope/name/sub", "@a/@b"] {
            validate(id)
                .unwrap_or_else(|e| panic!("`{id}` should be valid: {e}"));
        }
    }

    #[test]
    fn accepts_names_that_merely_resemble_reserved_ones() {
        // Reserved-name detection is exact on the stem, so look-alikes pass.
        for id in [
            "console",
            "com",
            "com0",
            "com10",
            "lpt0",
            "lpt10",
            "nul2",
            "prnx",
            "auxiliary",
        ] {
            validate(id)
                .unwrap_or_else(|e| panic!("`{id}` should be valid: {e}"));
        }
    }

    #[test]
    fn rejects_empty_id() {
        assert!(validate("").is_err());
    }

    #[test]
    fn rejects_backslash_separator() {
        let err = validate("a\\b").expect_err("backslash must be rejected");
        assert!(
            err.contains('/'),
            "error should point at the `/` separator rule: {err}"
        );
    }

    #[test]
    fn rejects_absolute_ids() {
        for id in ["/abs", "//a", "/"] {
            assert!(validate(id).is_err(), "`{id}` should be rejected");
        }
    }

    #[test]
    fn rejects_empty_segments() {
        for id in ["a//b", "a/", "a/b/"] {
            assert!(validate(id).is_err(), "`{id}` should be rejected");
        }
    }

    #[test]
    fn rejects_dot_segments() {
        for id in [".", "..", "a/./b", "a/../b", "../a"] {
            assert!(validate(id).is_err(), "`{id}` should be rejected");
        }
    }

    #[test]
    fn rejects_leading_and_trailing_punctuation() {
        for id in [".hidden", "abc.", "-abc", "abc-", "_abc", "abc_"] {
            assert!(validate(id).is_err(), "`{id}` should be rejected");
        }
    }

    #[test]
    fn rejects_illegal_characters() {
        for id in ["a<b>", "a:b", "a|b", "a?b", "a*b", "a b", "a@b", "a\"b"] {
            assert!(validate(id).is_err(), "`{id}` should be rejected");
        }
    }

    #[test]
    fn rejects_reserved_device_names_case_insensitively() {
        for id in ["con", "CON", "Con", "prn", "aux", "nul", "NUL"] {
            assert!(validate(id).is_err(), "`{id}` should be rejected");
        }
    }

    #[test]
    fn rejects_numbered_reserved_device_names() {
        for n in 1..=9 {
            for id in [format!("com{n}"), format!("LPT{n}")] {
                assert!(validate(&id).is_err(), "`{id}` should be rejected");
            }
        }
    }

    #[test]
    fn rejects_reserved_names_with_extensions() {
        for id in ["con.txt", "COM1.md", "nul.tar.gz"] {
            assert!(validate(id).is_err(), "`{id}` should be rejected");
        }
    }

    #[test]
    fn rejects_reserved_names_in_any_segment() {
        for id in ["a/con", "@scope/prn", "ok/nul/more"] {
            assert!(validate(id).is_err(), "`{id}` should be rejected");
        }
    }

    #[test]
    fn is_windows_reserved_name_matches_exact_and_numbered_devices() {
        for name in ["con", "PRN", "Aux", "nul", "com1", "COM9", "lpt5"] {
            assert!(
                is_windows_reserved_name(name),
                "`{name}` should be reserved"
            );
        }
        for name in ["con.txt", "NUL.dat"] {
            assert!(
                is_windows_reserved_name(name),
                "`{name}` should be reserved by stem"
            );
        }
    }

    #[test]
    fn is_windows_reserved_name_excludes_boundaries() {
        for name in [
            "", "com", "com0", "com10", "lpt0", "lpt10", "console", "nul2",
            "coms",
        ] {
            assert!(
                !is_windows_reserved_name(name),
                "`{name}` should not be reserved"
            );
        }
    }
}
