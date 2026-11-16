//! Filesystem-safe path segments for text that is not ours (RFC 0030, section
//! 4.1.1). The server builds storage keys with it and the client builds local
//! cache partition directories with the same rule, so the two always agree.

use sha2::{Digest as _, Sha256};

/// The longest scope label, in characters.
pub const MAX_SCOPE_LEN: usize = 64;

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        })
}

/// Windows treats these as devices, with or without an extension.
///
/// `conin$`, `conout$` and `clock$` cannot occur: `$` is not a safe-literal
/// character.
fn is_device_name(lowercase: &str) -> bool {
    let stem = lowercase.split('.').next().unwrap_or(lowercase);
    matches!(stem, "con" | "prn" | "aux" | "nul")
        || ["com", "lpt"].iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|rest| {
                rest.len() == 1 && matches!(rest.as_bytes()[0], b'1'..=b'9')
            })
        })
}

/// True when `text` can be a path segment as it is.
///
/// Only lowercase text qualifies: macOS and Windows file systems ignore case,
/// so `Ab` and `ab` would be one directory. Windows also drops a trailing `.`
/// and reserves device names, and a segment that starts with `h-` would be
/// indistinguishable from a hashed one. Everything else is hashed.
pub fn is_safe_literal(text: &str) -> bool {
    let bytes = text.as_bytes();
    let (Some(&first), Some(&last)) = (bytes.first(), bytes.last()) else {
        return false;
    };
    bytes.len() <= 64
        && matches!(first, b'a'..=b'z' | b'0'..=b'9')
        && bytes.iter().all(
            |b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-'),
        )
        && last != b'.'
        && !text.starts_with("h-")
        && !is_device_name(text)
}

/// The path segment for `text`: the text itself when it is a safe literal,
/// otherwise `h-` and 32 lowercase hex characters of its SHA-256.
///
/// This is part of the storage layout and must stay stable: operator tools
/// (the sweeper, purge) recompute a principal's segment with it. Text is hashed
/// byte for byte, so two Unicode normalization forms of one name are two
/// principals, never one merged namespace.
///
/// The two forms cannot meet (a literal never starts with `h-`, a hashed
/// segment always does), the hashed form is lowercase hex, and the mapping is
/// injective for literals and collision-resistant for the rest, so two
/// principals or scopes never share a directory, whatever the file system
/// does with case.
pub fn segment(text: &str) -> String {
    if is_safe_literal(text) {
        text.to_owned()
    } else {
        format!("h-{}", &hex(&Sha256::digest(text.as_bytes()))[..32])
    }
}

/// True for the scope labels the RFC allows: `[A-Za-z0-9][A-Za-z0-9._-]{0,63}`.
pub fn is_valid_scope_label(label: &str) -> bool {
    let bytes = label.as_bytes();
    matches!(bytes.first(), Some(b) if b.is_ascii_alphanumeric())
        && bytes.len() <= MAX_SCOPE_LEN
        && bytes.iter().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_are_kept_and_everything_else_is_hashed_stably() {
        assert_eq!(segment("ci"), "ci");
        assert_eq!(segment("Owner"), "h-4b1b8aa3608a26da451ae0630d75b60a");
        for text in ["con", "NUL.txt", "a.", "h-abc", "..", "a/b", "é", ""] {
            let s = segment(text);
            assert!(s.starts_with("h-") && s.len() == 34, "{text:?} -> {s}");
        }
        assert!(is_valid_scope_label("pr-12") && !is_valid_scope_label("../x"));
    }
}
