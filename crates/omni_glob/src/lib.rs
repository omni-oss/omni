use std::{path::Path, sync::Arc};

use globset::GlobSet;
use omni_utils::glob::build_glob_set_with;

pub use omni_utils::glob::GlobOptions;

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
        let include_patterns = patterns
            .iter()
            .filter(|p| is_include(p.as_ref()))
            .map(|p| transform(strip_bang(p.as_ref())))
            .collect::<Vec<_>>();

        let exclude_patterns = patterns
            .iter()
            .filter(|p| !is_include(p.as_ref()))
            .map(|p| transform(strip_bang(p.as_ref())))
            .collect::<Vec<_>>();

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
/// A pattern is an exclude when it has an odd number of leading `!`; an even
/// count (including zero) is an include, so `!!keep` is an include and `!drop`
/// is an exclude. This is the single source of truth for the `!` convention.
pub fn is_include(pattern: &str) -> bool {
    count_leading_bangs(pattern) % 2 == 0
}

fn strip_bang(mut s: &str) -> &str {
    while let Some(stripped) = s.strip_prefix('!') {
        s = stripped;
    }
    s
}

fn count_leading_bangs(s: &str) -> usize {
    s.chars().take_while(|c| *c == '!').count()
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

    #[test]
    fn count_leading_bangs_counts_only_the_prefix() {
        assert_eq!(count_leading_bangs("abc"), 0);
        assert_eq!(count_leading_bangs("!abc"), 1);
        assert_eq!(count_leading_bangs("!!abc"), 2);
        assert_eq!(count_leading_bangs("a!b"), 0);
    }

    #[test]
    fn strip_bang_removes_only_leading_bangs() {
        assert_eq!(strip_bang("abc"), "abc");
        assert_eq!(strip_bang("!abc"), "abc");
        assert_eq!(strip_bang("!!abc"), "abc");
        assert_eq!(strip_bang("a!b"), "a!b");
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
}
