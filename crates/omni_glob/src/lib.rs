use std::{
    borrow::Cow,
    path::{Path, PathBuf},
    sync::Arc,
};

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

/// Render a value as a glob pattern string.
///
/// Implementors hand back the exact text compiled into a [`GlobSet`]. Path-like
/// types normalize their separators to `/` so a pattern compiles the same way
/// on every platform.
pub trait ToGlobPattern {
    fn to_glob_pattern(&self) -> Cow<'_, str>;
}

impl ToGlobPattern for str {
    fn to_glob_pattern(&self) -> Cow<'_, str> {
        Cow::Borrowed(self)
    }
}

impl ToGlobPattern for String {
    fn to_glob_pattern(&self) -> Cow<'_, str> {
        Cow::Borrowed(self.as_str())
    }
}

impl ToGlobPattern for Path {
    fn to_glob_pattern(&self) -> Cow<'_, str> {
        normalize_separators(self.to_string_lossy())
    }
}

impl ToGlobPattern for PathBuf {
    fn to_glob_pattern(&self) -> Cow<'_, str> {
        normalize_separators(self.to_string_lossy())
    }
}

/// The display form of an [`OmniPath`] is its pattern. A rooted `@workspace/...`
/// path still resolves against the root map in the consumer before matching.
///
/// [`OmniPath`]: omni_types::OmniPath
impl ToGlobPattern for omni_types::OmniPath {
    fn to_glob_pattern(&self) -> Cow<'_, str> {
        Cow::Owned(self.to_string())
    }
}

fn normalize_separators(s: Cow<'_, str>) -> Cow<'_, str> {
    if s.contains('\\') {
        Cow::Owned(s.replace('\\', "/"))
    } else {
        s
    }
}

/// An include set and an exclude set of the same pattern kind.
///
/// A file is selected when it matches at least one include pattern and no
/// exclude pattern. The two sides are unordered and exclude always wins.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GlobPatterns<T> {
    pub include: Vec<T>,
    pub exclude: Vec<T>,
}

impl<T: ToGlobPattern> GlobPatterns<T> {
    /// The include and exclude pattern strings, ready for [`include_set`] or
    /// [`GlobMatcher::from_globs`]. When `T` is an `OmniPath`, the consumer
    /// resolves rooted paths against the root map before calling this.
    pub fn to_pattern_strings(&self) -> (Vec<String>, Vec<String>) {
        let render = |xs: &[T]| {
            xs.iter()
                .map(|t| t.to_glob_pattern().into_owned())
                .collect()
        };
        (render(&self.include), render(&self.exclude))
    }
}

#[derive(Debug, Clone)]
pub struct GlobMatcher {
    include: Arc<GlobSet>,
    exclude: Arc<GlobSet>,
}

impl GlobMatcher {
    /// Build a matcher from an explicit include set and exclude set. Patterns
    /// are compiled as written and matched against whatever path is handed to
    /// [`GlobMatcher::is_match`]. A leading `!` is an ordinary character.
    pub fn from_globs<S1: AsRef<str>, S2: AsRef<str>>(
        include: &[S1],
        exclude: &[S2],
        opts: GlobOptions,
    ) -> Result<Self, globset::Error> {
        Ok(Self {
            include: build_glob_set_with(include, opts)?,
            exclude: build_glob_set_with(exclude, opts)?,
        })
    }

    /// Build a matcher from an explicit include and exclude set, each pattern
    /// prefixed with `root` and matched against absolute paths.
    pub fn from_globs_rooted<P, S1, S2>(
        root: P,
        include: &[S1],
        exclude: &[S2],
        opts: GlobOptions,
    ) -> Result<Self, globset::Error>
    where
        P: AsRef<Path>,
        S1: AsRef<str>,
        S2: AsRef<str>,
    {
        let root = normalize_root(root.as_ref());
        let prefix = |p: &str| format!("{root}/{p}");
        let include: Vec<String> =
            include.iter().map(|p| prefix(p.as_ref())).collect();
        let exclude: Vec<String> =
            exclude.iter().map(|p| prefix(p.as_ref())).collect();

        Ok(Self {
            include: build_glob_set_with(&include, opts)?,
            exclude: build_glob_set_with(&exclude, opts)?,
        })
    }

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

fn normalize_root(root: &Path) -> String {
    let root = root.to_string_lossy();
    if cfg!(windows) && root.contains('\\') {
        root.replace('\\', "/")
    } else {
        root.to_string()
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
    fn from_globs_matches_include_and_not_exclude() {
        let m =
            GlobMatcher::from_globs(&["src/**/*.rs"], &["src/gen/**"], opts())
                .unwrap();

        assert!(m.is_match("src/a.rs"));
        assert!(m.is_match("src/nested/b.rs"));
        assert!(!m.is_match("src/gen/x.rs"));
        assert!(!m.is_match("src/a.txt"));
    }

    #[test]
    fn from_globs_exclude_wins_regardless_of_order() {
        // The same file is named on both sides; exclude always wins, and the
        // order the two sets are built in does not change the outcome.
        let m =
            GlobMatcher::from_globs(&["src/**"], &["src/**"], opts()).unwrap();

        assert!(!m.is_match("src/a.rs"));
    }

    #[test]
    fn from_globs_leading_bang_is_a_literal_character() {
        let m = GlobMatcher::from_globs(
            &["!keep.rs", "src/*"],
            &["!drop.rs"],
            opts(),
        )
        .unwrap();

        assert!(m.is_match("!keep.rs"));
        assert!(m.is_match("src/a.rs"));
        assert!(!m.is_match("!drop.rs"));
        // A plain `keep.rs` is not the literal `!keep.rs`, so it does not match.
        assert!(!m.is_match("keep.rs"));
    }

    #[test]
    fn from_globs_rooted_matches_relative_to_root_with_exclude() {
        let m = GlobMatcher::from_globs_rooted(
            "/ws",
            &["src/**/*.rs"],
            &["src/gen/**"],
            opts(),
        )
        .unwrap();

        assert!(m.is_match("/ws/src/a.rs"));
        assert!(!m.is_match("/ws/src/gen/x.rs"));
        assert!(!m.is_match("/other/src/a.rs"));
    }

    #[test]
    fn to_pattern_strings_renders_both_sides() {
        let patterns = GlobPatterns {
            include: vec!["src/**".to_string(), "docs/**".to_string()],
            exclude: vec!["src/gen/**".to_string()],
        };

        let (include, exclude) = patterns.to_pattern_strings();
        assert_eq!(include, vec!["src/**", "docs/**"]);
        assert_eq!(exclude, vec!["src/gen/**"]);
    }

    #[test]
    fn to_pattern_strings_normalizes_path_separators() {
        let patterns = GlobPatterns {
            include: vec![PathBuf::from("src").join("lib").join("**")],
            exclude: vec![PathBuf::from("src").join("gen")],
        };

        let (include, exclude) = patterns.to_pattern_strings();
        assert_eq!(include, vec!["src/lib/**"]);
        assert_eq!(exclude, vec!["src/gen"]);
    }

    #[test]
    fn omni_path_renders_its_display_form() {
        use omni_types::{OmniPath, Root};

        let rooted = OmniPath::new_rooted("src/**", Root::Workspace);
        assert_eq!(rooted.to_glob_pattern(), "@workspace/src/**");

        let plain = OmniPath::<Root>::new("src/**");
        assert_eq!(plain.to_glob_pattern(), "src/**");
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
