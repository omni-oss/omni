use std::{path::Path, sync::Arc};

use globset::GlobSet;
use omni_utils::glob::build_glob_set_with;

pub use omni_utils::glob::GlobOptions;

/// Build a memoized, include-only compiled [`GlobSet`] for `patterns`.
///
/// Patterns are matched exactly as written: a leading `!` is a literal
/// character, never a negation marker. This is the entry point for callers that
/// deliberately do not want negation, above all the hashed paths, where reading
/// `!` as an exclude would silently change a cache key. Callers that do want
/// negation use [`GlobMatcher`] instead.
///
/// The result shares the same process-wide cache as the matcher, so a set built
/// here is the same [`Arc`] as one built for the same patterns and options
/// elsewhere.
pub fn include_set<S: AsRef<str>>(
    patterns: &[S],
    opts: GlobOptions,
) -> Result<Arc<GlobSet>, globset::Error> {
    build_glob_set_with(patterns, opts)
}

#[derive(Debug, Clone)]
pub struct GlobMatcher {
    include: Arc<GlobSet>,
    exclude: Arc<GlobSet>,
}

impl GlobMatcher {
    /// Build a matcher whose patterns are matched as written against whatever
    /// path is handed to [`GlobMatcher::is_match`] (for example a path relative
    /// to a source root). Leading `!` markers are stripped before compilation.
    pub fn new<S: AsRef<str>>(
        patterns: &[S],
        opts: GlobOptions,
    ) -> Result<Self, globset::Error> {
        Self::from_patterns(patterns, opts, |p| p.to_owned())
    }

    /// Build a matcher whose patterns are each prefixed with `root` and matched
    /// against absolute paths.
    pub fn rooted<P: AsRef<Path>, S: AsRef<str>>(
        root: P,
        patterns: &[S],
        opts: GlobOptions,
    ) -> Result<Self, globset::Error> {
        let root = root.as_ref().to_string_lossy();
        let root = if cfg!(windows) && root.contains('\\') {
            root.replace('\\', "/")
        } else {
            root.to_string()
        };

        Self::from_patterns(patterns, opts, |p| format!("{root}/{p}"))
    }

    fn from_patterns<S: AsRef<str>>(
        patterns: &[S],
        opts: GlobOptions,
        transform: impl Fn(&str) -> String,
    ) -> Result<Self, globset::Error> {
        let mut include_patterns = Vec::new();
        let mut exclude_patterns = Vec::new();

        for pattern in patterns {
            let marker = scan_markers(pattern.as_ref());
            let transformed = transform(&marker.body);
            if marker.bangs.is_multiple_of(2) {
                include_patterns.push(transformed);
            } else {
                exclude_patterns.push(transformed);
            }
        }

        Ok(Self {
            include: build_glob_set_with(&include_patterns, opts)?,
            exclude: build_glob_set_with(&exclude_patterns, opts)?,
        })
    }

    pub fn is_match<P: AsRef<Path>>(&self, path: P) -> bool {
        self.include.is_match(&path) && !self.exclude.is_match(&path)
    }
}

/// Classify a raw pattern as an include (`true`) or an exclude (`false`).
///
/// A pattern is an exclude when its leading marker run holds an odd number of
/// `!`; an even count (including zero) is an include, so `!!keep` is an include
/// and `!drop` is an exclude. A `\!` in that run is a literal `!`, not a marker.
/// This is the single source of truth for the `!` convention.
pub fn is_include(pattern: &str) -> bool {
    scan_markers(pattern).bangs.is_multiple_of(2)
}

/// The leading marker run of a pattern, resolved.
struct Marker {
    /// Number of unescaped leading `!` markers.
    bangs: usize,
    /// The pattern with its leading marker run resolved: the `!` markers
    /// dropped and each `\!` escape reduced to a literal `!`.
    body: String,
}

/// Scan the leading run of `!` and `\!` markers at the start of `pattern`.
///
/// Walking from the start, an unescaped `!` is one negation marker, a `\!` pair
/// is a literal `!` in the body (the `\` is dropped), and the first other
/// character ends the run. Everything from that point is copied verbatim.
///
/// Only the exact pair `\!` is special. A `\` not followed by `!` is left
/// alone, so `\\` does not escape a literal backslash and a mid-pattern `\!`
/// is untouched. Resolving the escape here, before globset compiles the body,
/// is what makes it behave the same on every platform.
fn scan_markers(pattern: &str) -> Marker {
    let bytes = pattern.as_bytes();
    let mut i = 0;
    let mut bangs = 0;
    let mut body = String::with_capacity(pattern.len());

    while i < bytes.len() {
        if bytes[i] == b'!' {
            bangs += 1;
            i += 1;
        } else if bytes[i] == b'\\' && bytes.get(i + 1) == Some(&b'!') {
            body.push('!');
            i += 2;
        } else {
            break;
        }
    }

    // `i` only ever advances past ASCII `!` and `\`, so it lands on a char
    // boundary and this slice is always valid.
    body.push_str(&pattern[i..]);

    Marker { bangs, body }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> GlobOptions {
        GlobOptions::default()
    }

    #[test]
    fn is_include_follows_bang_parity() {
        assert!(is_include("keep.rs"));
        assert!(!is_include("!drop.rs"));
        assert!(is_include("!!keep.rs"));
        assert!(!is_include("!!!drop.rs"));
    }

    // The escape table: for each pattern, its bang parity (include vs exclude)
    // and the body handed to globset. Enforced on both platforms, since the
    // whole point of resolving `\!` before globset is that it no longer depends
    // on globset's platform-dependent backslash handling.
    #[test]
    fn scan_markers_resolves_the_escape_table() {
        let cases: &[(&str, bool, &str)] = &[
            ("foo", true, "foo"),
            ("!foo", false, "foo"),
            ("!!foo", true, "foo"),
            ("\\!foo", true, "!foo"),
            ("!\\!foo", false, "!foo"),
            ("\\!\\!foo", true, "!!foo"),
            ("\\!!foo", false, "!foo"),
            ("\\foo", true, "\\foo"),
            ("foo\\!bar", true, "foo\\!bar"),
            ("\\!", true, "!"),
            ("!", false, ""),
            ("\\\\!foo", true, "\\\\!foo"),
        ];

        for (pattern, expected_include, expected_body) in cases {
            let marker = scan_markers(pattern);
            assert_eq!(
                marker.bangs.is_multiple_of(2),
                *expected_include,
                "include bit for pattern: {pattern:?}"
            );
            assert_eq!(
                marker.body, *expected_body,
                "body for pattern: {pattern:?}"
            );
        }
    }

    #[test]
    fn escaped_leading_bang_matches_a_literal_bang_name() {
        let m = GlobMatcher::new(&["\\!keep.rs", "src/*"], opts()).unwrap();

        assert!(m.is_match("!keep.rs"));
        assert!(m.is_match("src/a.rs"));
        assert!(!m.is_match("keep.rs"));
    }

    #[test]
    fn escaped_then_negated_excludes_a_literal_bang_name() {
        // `!\!drop.rs` is one real negation of the literal name `!drop.rs`.
        let m = GlobMatcher::new(&["*", "!\\!drop.rs"], opts()).unwrap();

        assert!(m.is_match("keep.rs"));
        assert!(!m.is_match("!drop.rs"));
    }

    #[test]
    fn rooted_matches_included_paths_relative_to_root() {
        let m = GlobMatcher::rooted("/ws", &["src/**/*.rs"], opts()).unwrap();

        assert!(m.is_match("/ws/src/a.rs"));
        assert!(m.is_match("/ws/src/nested/b.rs"));
        assert!(!m.is_match("/ws/src/a.txt"));
        assert!(!m.is_match("/other/src/a.rs"));
    }

    #[test]
    fn rooted_excludes_negated_patterns() {
        let m =
            GlobMatcher::rooted("/ws", &["src/**/*.rs", "!src/gen/**"], opts())
                .unwrap();

        assert!(m.is_match("/ws/src/a.rs"));
        assert!(!m.is_match("/ws/src/gen/x.rs"));
    }

    #[test]
    fn rooted_double_negation_is_an_include() {
        let m = GlobMatcher::rooted("/ws", &["!!keep.rs"], opts()).unwrap();

        assert!(m.is_match("/ws/keep.rs"));
    }

    #[test]
    fn rooted_nothing_matches_without_include_patterns() {
        let m = GlobMatcher::rooted("/ws", &["!src/**"], opts()).unwrap();

        assert!(!m.is_match("/ws/src/a.rs"));
        assert!(!m.is_match("/ws/other.rs"));
    }

    #[test]
    fn new_matches_patterns_as_written() {
        let m =
            GlobMatcher::new(&["src/**/*.rs", "!src/gen/**"], opts()).unwrap();

        assert!(m.is_match("src/a.rs"));
        assert!(m.is_match("src/nested/b.rs"));
        assert!(!m.is_match("src/gen/x.rs"));
        assert!(!m.is_match("src/a.txt"));
    }

    #[test]
    fn literal_separator_stops_star_at_slash() {
        let m = GlobMatcher::new(
            &["src/*"],
            GlobOptions {
                literal_separator: true,
            },
        )
        .unwrap();

        assert!(m.is_match("src/a.rs"));
        assert!(!m.is_match("src/nested/a.rs"));
    }

    // Patterns taken from the four build_glob_set callers (collector
    // input/output globs, project-hasher task names, filter and get_stats
    // globs) and the GlobMatcher consumers (discovery, generator, projections).
    // None contains `\!` in its leading run of `!`, so the escape rewrite must
    // not change how any of them classify.
    #[test]
    fn no_regression_corpus_classification() {
        let cases: &[(&str, bool)] = &[
            ("src/**/*.rs", true),
            ("dist/**", true),
            ("@workspace/shared/**/*.toml", true),
            ("build", true),
            ("*", true),
            ("project-*", true),
            ("!src/gen/**", false),
            ("!!keep.rs", true),
            ("!!!drop.rs", false),
            ("\\foo", true),
            ("foo\\!bar", true),
        ];

        for (pattern, expected_include) in cases {
            assert_eq!(
                is_include(pattern),
                *expected_include,
                "pattern: {pattern}"
            );
        }
    }

    #[test]
    fn no_regression_corpus_matching() {
        let m = GlobMatcher::new(
            &["src/**/*.rs", "!src/gen/**", "!!keep.rs"],
            opts(),
        )
        .unwrap();

        assert!(m.is_match("src/a.rs"));
        assert!(m.is_match("src/nested/b.rs"));
        assert!(!m.is_match("src/gen/x.rs"));
        assert!(m.is_match("keep.rs"));
        assert!(!m.is_match("src/a.txt"));
    }

    #[test]
    fn identical_patterns_share_one_compiled_set() {
        let a = omni_utils::glob::build_glob_set(&["src/**/*.rs", "dist/**"])
            .unwrap();
        let b = omni_utils::glob::build_glob_set(&["src/**/*.rs", "dist/**"])
            .unwrap();

        assert!(std::sync::Arc::ptr_eq(&a, &b));
    }

    // include_set is a thin re-surfacing of the memoized builder, so a set built
    // through it is the very same Arc as one built through build_glob_set. This
    // is what makes the caller migration byte-identical.
    #[test]
    fn include_set_aliases_the_memoized_builder() {
        let patterns = ["!literal.rs", "src/**/*.rs"];

        let via_include =
            include_set(&patterns, GlobOptions::default()).unwrap();
        let via_builder = omni_utils::glob::build_glob_set(&patterns).unwrap();

        assert!(std::sync::Arc::ptr_eq(&via_include, &via_builder));

        // The leading `!` is a literal character here, not a negation: the set
        // matches the name `!literal.rs`, and does not turn into an exclude.
        assert!(via_include.is_match("!literal.rs"));
        assert!(via_include.is_match("src/a.rs"));
    }
}
