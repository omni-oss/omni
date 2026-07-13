use std::{path::Path, sync::Arc};

use globset::GlobSet;
use omni_utils::glob::build_glob_set;

#[derive(Debug, Clone)]
pub struct GlobMatcher {
    include_matcher: Arc<GlobSet>,
    exclude_matcher: Arc<GlobSet>,
}

impl GlobMatcher {
    pub fn new<P: AsRef<Path>, S: AsRef<str>>(
        root_dir: P,
        glob_patterns: &[S],
    ) -> Result<Self, globset::Error> {
        let root_dir = root_dir.as_ref().to_string_lossy();
        let root = if cfg!(windows) && root_dir.contains('\\') {
            root_dir.replace('\\', "/")
        } else {
            root_dir.to_string()
        };

        let include_patterns = glob_patterns
            .iter()
            .filter(|p| count_starts_with(p.as_ref(), "!") % 2 == 0)
            .map(|p| format!("{root}/{}", strip_starts_with(p.as_ref(), "!")))
            .collect::<Vec<_>>();

        let include_matcher = build_glob_set(&include_patterns)?;

        let exclude_patterns = glob_patterns
            .iter()
            .filter(|p| count_starts_with(p.as_ref(), "!") % 2 == 1)
            .map(|p| format!("{root}/{}", strip_starts_with(p.as_ref(), "!")))
            .collect::<Vec<_>>();

        let exclude_matcher = build_glob_set(&exclude_patterns)?;

        Ok(Self {
            include_matcher,
            exclude_matcher,
        })
    }
}

impl GlobMatcher {
    pub fn is_match<P: AsRef<Path>>(&self, path: P) -> bool {
        self.include_matcher.is_match(&path)
            && !self.exclude_matcher.is_match(&path)
    }
}

fn count_starts_with(mut s: &str, prefix: &str) -> usize {
    if prefix.is_empty() {
        return s.len();
    }

    let mut count = 0;
    let prefix_len = prefix.len();

    while let Some(pos) = s.find(prefix) {
        if pos == 0 {
            count += 1;
        }
        s = &s[pos + prefix_len..];
    }

    count
}

fn strip_starts_with<'a>(mut s: &'a str, prefix: &str) -> &'a str {
    if prefix.is_empty() {
        return s;
    }
    while let Some(stripped) = s.strip_prefix(prefix) {
        s = stripped;
    }

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_starts_with() {
        assert_eq!(count_starts_with("abc", "a"), 1);
        assert_eq!(count_starts_with("abc", "ab"), 1);
        assert_eq!(count_starts_with("abc", "abc"), 1);
        assert_eq!(count_starts_with("abc", "b"), 0);
        assert_eq!(count_starts_with("abc", "c"), 0);
        assert_eq!(count_starts_with("abc", ""), 3);
    }

    #[test]
    fn test_strip_starts_with() {
        assert_eq!(strip_starts_with("abc", "a"), "bc");
        assert_eq!(strip_starts_with("abc", "ab"), "c");
        assert_eq!(strip_starts_with("abc", "abc"), "");
        assert_eq!(strip_starts_with("abc", "b"), "abc");
        assert_eq!(strip_starts_with("abc", "c"), "abc");
        assert_eq!(strip_starts_with("abc", ""), "abc");
    }

    #[test]
    fn matches_included_paths_relative_to_root() {
        let m = GlobMatcher::new("/ws", &["src/**/*.rs"]).unwrap();

        assert!(m.is_match("/ws/src/a.rs"));
        assert!(m.is_match("/ws/src/nested/b.rs"));
        assert!(!m.is_match("/ws/src/a.txt"));
        assert!(!m.is_match("/other/src/a.rs"));
    }

    #[test]
    fn excludes_negated_patterns() {
        let m =
            GlobMatcher::new("/ws", &["src/**/*.rs", "!src/gen/**"]).unwrap();

        assert!(m.is_match("/ws/src/a.rs"));
        // Included by the first pattern but removed by the negated one.
        assert!(!m.is_match("/ws/src/gen/x.rs"));
    }

    #[test]
    fn double_negation_is_an_include() {
        // An even number of leading `!` cancels out, so this is an include.
        let m = GlobMatcher::new("/ws", &["!!keep.rs"]).unwrap();

        assert!(m.is_match("/ws/keep.rs"));
    }

    #[test]
    fn nothing_matches_without_include_patterns() {
        let m = GlobMatcher::new("/ws", &["!src/**"]).unwrap();

        assert!(!m.is_match("/ws/src/a.rs"));
        assert!(!m.is_match("/ws/other.rs"));
    }
}
