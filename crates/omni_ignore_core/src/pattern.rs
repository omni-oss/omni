use std::path::Path;

use crate::{error::IgnoreError, fence};

/// One rendered gitignore line: already anchored, escaped, and (for a keep-rule)
/// negation-prefixed. The engine only orders and joins patterns, never rewrites
/// them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IgnorePattern(String);

impl IgnorePattern {
    /// A concrete workspace-relative path, anchored with a leading `/` and with
    /// gitignore-significant characters escaped so a literal path is never read
    /// as a comment, negation, or character class.
    pub fn path(rel: &Path) -> Self {
        let normalized = rel.to_string_lossy().replace('\\', "/");
        let mut line = String::with_capacity(normalized.len() + 1);
        if !normalized.starts_with('/') {
            line.push('/');
        }
        for ch in normalized.chars() {
            if matches!(ch, '#' | '!' | '[') {
                line.push('\\');
            }
            line.push(ch);
        }
        escape_trailing_spaces(&mut line);
        Self(line)
    }

    /// A raw authored line (globs, `**`, a leading `!`), validated as a single
    /// non-fence line.
    pub fn raw(line: &str) -> Result<Self, IgnoreError> {
        if line.is_empty() {
            return Err(IgnoreError::EmptyPattern);
        }
        if line.contains('\n') || line.contains('\r') {
            return Err(IgnoreError::MultiLinePattern(line.to_string()));
        }
        if fence::is_fence_line(line) {
            return Err(IgnoreError::FenceInPattern(line.to_string()));
        }
        Ok(Self(line.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn escape_trailing_spaces(line: &mut String) {
    let trailing = line.len() - line.trim_end_matches(' ').len();
    if trailing == 0 {
        return;
    }
    line.truncate(line.len() - trailing);
    for _ in 0..trailing {
        line.push_str("\\ ");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_anchors_a_relative_path() {
        assert_eq!(
            IgnorePattern::path(Path::new(".agents/skills/unslop")).as_str(),
            "/.agents/skills/unslop"
        );
    }

    #[test]
    fn path_keeps_a_single_leading_slash() {
        assert_eq!(
            IgnorePattern::path(Path::new("/already/anchored")).as_str(),
            "/already/anchored"
        );
    }

    #[test]
    fn path_normalizes_backslashes_to_forward_slashes() {
        assert_eq!(
            IgnorePattern::path(Path::new("a\\b\\c")).as_str(),
            "/a/b/c"
        );
    }

    #[test]
    fn path_escapes_comment_negation_and_class_characters() {
        assert_eq!(
            IgnorePattern::path(Path::new("weird/#!both[x")).as_str(),
            "/weird/\\#\\!both\\[x"
        );
    }

    #[test]
    fn path_escapes_trailing_spaces() {
        assert_eq!(
            IgnorePattern::path(Path::new("trailing  ")).as_str(),
            "/trailing\\ \\ "
        );
    }

    #[test]
    fn raw_accepts_a_glob_and_a_negation() {
        assert_eq!(
            IgnorePattern::raw("/.omni/sources/*/**").unwrap().as_str(),
            "/.omni/sources/*/**"
        );
        assert_eq!(
            IgnorePattern::raw("!/.omni/sources/*/lock.json")
                .unwrap()
                .as_str(),
            "!/.omni/sources/*/lock.json"
        );
    }

    #[test]
    fn raw_rejects_empty_multiline_and_fence_input() {
        assert!(matches!(
            IgnorePattern::raw(""),
            Err(IgnoreError::EmptyPattern)
        ));
        assert!(matches!(
            IgnorePattern::raw("a\nb"),
            Err(IgnoreError::MultiLinePattern(_))
        ));
        assert!(matches!(
            IgnorePattern::raw("a\r"),
            Err(IgnoreError::MultiLinePattern(_))
        ));
        assert!(matches!(
            IgnorePattern::raw(fence::FENCE_BEGIN),
            Err(IgnoreError::FenceInPattern(_))
        ));
        assert!(matches!(
            IgnorePattern::raw("# @@omni-managed:end 3f9a1c8e"),
            Err(IgnoreError::FenceInPattern(_))
        ));
    }
}
