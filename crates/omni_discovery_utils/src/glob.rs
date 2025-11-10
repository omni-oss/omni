use std::path::Path;

use globset::{Glob, GlobSetBuilder};

#[derive(Debug, Clone)]
pub struct GlobMatcher {
    include_matcher: globset::GlobSet,
    exclude_matcher: globset::GlobSet,
}

impl GlobMatcher {
    pub fn new<P: AsRef<Path>, S: AsRef<str>>(
        root_dir: P,
        glob_patterns: &[S],
    ) -> Result<Self, globset::Error> {
        let mut match_include = GlobSetBuilder::new();
        let root_dir = root_dir.as_ref().to_string_lossy();
        let root = if cfg!(windows) && root_dir.contains('\\') {
            root_dir.replace('\\', "/")
        } else {
            root_dir.to_string()
        };

        for p in glob_patterns
            .iter()
            .filter(|p| count_starts_with(p.as_ref(), "!") % 2 == 0)
        {
            let pat = format!("{root}/{}", strip_starts_with(p.as_ref(), "!"));

            trace::trace!("adding include pattern: {}", pat);

            match_include.add(Glob::new(pat.as_str())?);
        }

        let include_matcher = match_include.build()?;

        let mut match_exclude = GlobSetBuilder::new();

        for p in glob_patterns
            .iter()
            .filter(|p| count_starts_with(p.as_ref(), "!") % 2 == 1)
        {
            let pat = format!("{root}/{}", strip_starts_with(p.as_ref(), "!"));

            trace::trace!("adding exclude pattern: {}", pat);

            match_exclude.add(Glob::new(pat.as_str())?);
        }

        let exclude_matcher = match_exclude.build()?;

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
}
