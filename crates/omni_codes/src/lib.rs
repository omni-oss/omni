//! The canonical registry of omni's diagnostic and notice codes.
//!
//! Diagnostics (fatal errors) and notices (non-fatal messages) both register
//! here through their derives. Codes are unique across the whole application.
//! `omni code`, the MCP `code_explain` and `code_list` tools and the docs read this registry.

extern crate self as omni_codes;

pub mod catalog;
mod registry;
mod validate;

use std::fmt;

use serde_json::{Value, json};

#[cfg(feature = "explicit-registry")]
pub use registry::install;
pub use registry::{CodeRegistration, all, get, registrations};
pub use validate::{RegistryError, retired_codes, validate, validate_set};

/// Severity of a notice. Serialises lowercase (`"info"`, `"warn"`, `"error"`).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    strum::Display,
    strum::IntoStaticStr,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum NoticeLevel {
    Info,
    Warn,
    Error,
}

impl NoticeLevel {
    pub fn as_str(self) -> &'static str {
        self.into()
    }
}

/// Who is expected to act on a diagnostic.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    serde::Serialize,
    strum::Display,
    strum::IntoStaticStr,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum Audience {
    /// The input is wrong: arguments, config, expression, files.
    User,
    /// Environment or external system: network, disk, permissions.
    Environment,
    /// omni itself misbehaved.
    Internal,
}

impl Audience {
    pub fn as_str(self) -> &'static str {
        self.into()
    }

    /// Process exit code: `2` for user errors (same as clap usage errors),
    /// `1` for everything else.
    pub const fn exit_code(self) -> i32 {
        match self {
            Self::User => 2,
            Self::Environment | Self::Internal => 1,
        }
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    strum::Display,
    strum::IntoStaticStr,
)]
#[strum(serialize_all = "lowercase")]
pub enum CodeKind {
    Error,
    Notice,
}

impl CodeKind {
    pub fn as_str(self) -> &'static str {
        self.into()
    }
}

/// Kind-specific data. A notice has no audience and an error has no level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeDetail {
    Notice { level: NoticeLevel, essential: bool },
    Error { audience: Audience },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeInfo {
    pub code: &'static str,
    /// First doc line.
    pub summary: &'static str,
    /// Rest of the doc comment.
    pub explanation: &'static str,
    pub related: &'static [&'static str],
    /// Covered by the stability promise.
    pub stable: bool,
    pub deprecated: bool,
    pub detail: CodeDetail,
}

impl CodeInfo {
    pub const fn kind(&self) -> CodeKind {
        match self.detail {
            CodeDetail::Notice { .. } => CodeKind::Notice,
            CodeDetail::Error { .. } => CodeKind::Error,
        }
    }

    pub const fn level(&self) -> Option<NoticeLevel> {
        match self.detail {
            CodeDetail::Notice { level, .. } => Some(level),
            CodeDetail::Error { .. } => None,
        }
    }

    pub const fn audience(&self) -> Option<Audience> {
        match self.detail {
            CodeDetail::Error { audience } => Some(audience),
            CodeDetail::Notice { .. } => None,
        }
    }

    pub const fn essential(&self) -> bool {
        match self.detail {
            CodeDetail::Notice { essential, .. } => essential,
            CodeDetail::Error { .. } => false,
        }
    }

    /// The JSON form shared by `omni code explain --format json` and the MCP
    /// `code_explain` tool.
    pub fn to_json(&self) -> Value {
        let mut v = json!({
            "code": self.code,
            "kind": self.kind().as_str(),
            "summary": self.summary,
            "explanation": self.explanation,
            "essential": self.essential(),
            "stable": self.stable,
            "deprecated": self.deprecated,
            "related": self.related,
        });
        if let Some(level) = self.level() {
            v["level"] = json!(level);
        }
        if let Some(audience) = self.audience() {
            v["audience"] = json!(audience);
        }
        v
    }

    /// Plain-text form used by `omni code explain`.
    pub fn render_text(&self) -> String {
        use fmt::Write as _;
        let mut out = String::new();
        let _ = writeln!(out, "{}", self.code);
        let _ = writeln!(out, "  kind:       {}", self.kind().as_str());
        if let Some(l) = self.level() {
            let _ = writeln!(out, "  level:      {}", l.as_str());
        }
        if let Some(a) = self.audience() {
            let _ = writeln!(out, "  audience:   {a}");
        }
        let _ = writeln!(out, "  essential:  {}", self.essential());
        let _ = writeln!(out, "  stable:     {}", self.stable);
        if self.deprecated {
            let _ = writeln!(out, "  deprecated: true");
        }
        if !self.summary.is_empty() {
            let _ = writeln!(out, "\n{}", self.summary);
        }
        if !self.explanation.is_empty() {
            let _ = writeln!(out, "\n{}", self.explanation);
        }
        if !self.related.is_empty() {
            let _ = writeln!(out, "\nSee also: {}", self.related.join(", "));
        }
        out
    }
}

/// Implemented by both derives. Sealed: cannot be implemented by hand.
///
/// Has an associated const, so it is not object-safe.
pub trait Code: __private::Sealed {
    /// One entry for a notice, one per variant for a diagnostic enum.
    const CODES: &'static [CodeInfo];

    fn info(&self) -> &'static CodeInfo;
}

/// A code that is not in the registry, with the closest known codes.
#[derive(Debug, Clone)]
pub struct UnknownCode {
    pub code: String,
    pub close_matches: Vec<&'static CodeInfo>,
}

/// Resolves a code for `omni code explain` and the MCP `code_explain` tool.
/// An unknown code returns [`UnknownCode`] with suggested matches.
pub fn lookup(code: &str) -> Result<&'static CodeInfo, UnknownCode> {
    get(code).ok_or_else(|| UnknownCode {
        code: code.to_string(),
        close_matches: close_matches(code),
    })
}

/// Up to five codes close to `code`, for "did you mean" output, ordered by
/// edit distance and then code.
pub fn close_matches(code: &str) -> Vec<&'static CodeInfo> {
    let mut scored: Vec<(usize, &'static CodeInfo)> = all()
        .filter_map(|c| {
            let d = edit_distance(code, c.code);
            let same_prefix = code.split('.').next()
                == c.code.split('.').next()
                && code.contains('.');
            let contains = !code.is_empty()
                && (c.code.contains(code) || code.contains(c.code));
            (d <= 3 || same_prefix || contains).then_some((d, c))
        })
        .collect();
    scored.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.code.cmp(b.1.code)));
    scored.into_iter().map(|(_, c)| c).take(5).collect()
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// `true` for lowercase `snake_case` segments separated by dots, at least two.
pub fn valid_code(code: &str) -> bool {
    let mut segments = 0;
    for seg in code.split('.') {
        segments += 1;
        let mut chars = seg.chars();
        match chars.next() {
            Some(c) if c.is_ascii_lowercase() => {}
            _ => return false,
        }
        if !chars
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            return false;
        }
    }
    segments >= 2
}

#[doc(hidden)]
pub mod __private {
    /// Marker emitted only by the derives.
    pub trait Sealed {}

    #[cfg(all(feature = "inventory", not(feature = "explicit-registry")))]
    pub use inventory;
}

/// Submits a [`CodeRegistration`]. Emitted by the derives.
#[cfg(all(feature = "inventory", not(feature = "explicit-registry")))]
#[macro_export]
macro_rules! submit {
    ($e:expr) => {
        $crate::__private::inventory::submit! { $e }
    };
}

/// Explicit-registry backend: add the registration to the list given to
/// `install` instead.
#[cfg(feature = "explicit-registry")]
#[macro_export]
macro_rules! submit {
    ($e:expr) => {};
}

/// Declares the code prefixes the calling crate owns.
///
/// ```ignore
/// omni_codes::prefixes!("cache", "hash");
/// ```
#[macro_export]
macro_rules! prefixes {
    ($($p:literal),+ $(,)?) => {
        $crate::submit!($crate::CodeRegistration {
            crate_name: ::core::env!("CARGO_PKG_NAME"),
            codes: &[],
            prefixes: &[$($p),+],
        });
    };
}

#[cfg(not(any(feature = "inventory", feature = "explicit-registry")))]
compile_error!(
    "omni_codes needs the `inventory` or `explicit-registry` feature"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_format() {
        assert!(valid_code("cache.write_skipped"));
        assert!(valid_code("a.b.c"));
        assert!(!valid_code("cache"));
        assert!(!valid_code("Cache.x"));
        assert!(!valid_code("cache..x"));
        assert!(!valid_code("cache.1x"));
    }

    #[test]
    fn level_wire_format() {
        assert_eq!(
            serde_json::to_string(&NoticeLevel::Info).unwrap(),
            "\"info\""
        );
        assert_eq!(
            serde_json::to_string(&NoticeLevel::Warn).unwrap(),
            "\"warn\""
        );
        assert_eq!(
            serde_json::to_string(&NoticeLevel::Error).unwrap(),
            "\"error\""
        );
    }

    #[test]
    fn lookup_unknown_code_lists_matches_or_nothing() {
        let e = lookup("no.such_code").unwrap_err();
        assert_eq!(e.code, "no.such_code");
        // The unit-test binary registers nothing, so there is nothing close.
        assert!(e.close_matches.is_empty());
    }

    #[test]
    fn edit_distance_basics() {
        assert_eq!(edit_distance("abc", "abc"), 0);
        assert_eq!(edit_distance("abc", "abd"), 1);
        assert_eq!(edit_distance("", "abc"), 3);
    }
}
