use std::{collections::HashSet, sync::Arc};

use globset::GlobSet;
use omni_glob::{GlobOptions, include_set};

/// Selects names by exact match, by glob, or both. With neither, every name
/// matches. An exact name never acts as a glob, so `build*` selects only a
/// name that is literally `build*`.
#[derive(Clone)]
pub(crate) struct NameMatcher {
    exact: Arc<HashSet<String>>,
    patterns: Option<Arc<GlobSet>>,
    all: bool,
}

impl NameMatcher {
    pub(crate) fn new(
        exact: &[&str],
        patterns: &[&str],
    ) -> Result<Self, globset::Error> {
        Ok(Self {
            all: exact.is_empty() && patterns.is_empty(),
            exact: Arc::new(exact.iter().map(|n| n.to_string()).collect()),
            patterns: if patterns.is_empty() {
                None
            } else {
                Some(include_set(patterns, GlobOptions::default())?)
            },
        })
    }

    pub(crate) fn is_match(&self, name: &str) -> bool {
        self.all
            || self.exact.contains(name)
            || self.patterns.as_ref().is_some_and(|p| p.is_match(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matcher(exact: &[&str], patterns: &[&str]) -> NameMatcher {
        NameMatcher::new(exact, patterns).unwrap()
    }

    #[test]
    fn no_selectors_match_every_name() {
        let m = matcher(&[], &[]);
        assert!(m.is_match("build") && m.is_match("*") && m.is_match(""));
    }

    #[test]
    fn exact_names_are_literal() {
        let m = matcher(&["build*", "test"], &[]);
        assert!(m.is_match("build*") && m.is_match("test"));
        assert!(!m.is_match("build") && !m.is_match("builder"));
        assert!(!m.is_match("tests"));
    }

    #[test]
    fn patterns_are_globs_and_a_literal_star_is_not_a_pattern() {
        let m = matcher(&["*"], &[]);
        assert!(m.is_match("*") && !m.is_match("build"));
        let m = matcher(&[], &["*"]);
        assert!(m.is_match("*") && m.is_match("build"));
    }

    #[test]
    fn exact_and_patterns_union() {
        let m = matcher(&["lint"], &["te*"]);
        assert!(m.is_match("lint") && m.is_match("test") && m.is_match("tea"));
        assert!(!m.is_match("build"));
    }

    #[test]
    fn a_bad_pattern_is_an_error_not_a_match_all() {
        assert!(NameMatcher::new(&[], &["["]).is_err());
    }
}
