//! Registry checks that a proc-macro cannot do. Run these from a test in a
//! crate where every diagnostic-defining crate is linked (the `omni` binary).

use std::collections::HashMap;

use crate::registrations;

fn valid_code(code: &str) -> bool {
    let mut segments = 0;
    for seg in code.split('.') {
        segments += 1;
        let mut chars = seg.chars();
        if !chars.next().is_some_and(|c| c.is_ascii_lowercase()) {
            return false;
        }
        if !chars
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            return false;
        }
    }
    segments >= 2
}

/// Panics if any registered code is malformed or used by two types/variants.
pub fn assert_registry_valid() {
    let mut seen: HashMap<&str, &str> = HashMap::new();
    for r in registrations() {
        for c in r.codes {
            assert!(
                valid_code(c.code),
                "invalid diagnostic code `{}` on `{}`",
                c.code,
                r.type_name
            );
            if let Some(prev) = seen.insert(c.code, r.type_name) {
                panic!(
                    "diagnostic code `{}` is used by both `{prev}` and `{}`",
                    c.code, r.type_name
                );
            }
        }
    }
}

/// Panics if any of `expected` (type paths, matched by suffix) is not
/// registered. Catches crates dropped by the linker because nothing
/// references them.
pub fn assert_types_registered(expected: &[&str]) {
    let have: Vec<&str> = registrations().map(|r| r.type_name).collect();
    for e in expected {
        assert!(
            have.iter().any(|h| h.ends_with(e)),
            "diagnostic type `{e}` is not registered (is its crate linked?); registered: {have:?}"
        );
    }
}
