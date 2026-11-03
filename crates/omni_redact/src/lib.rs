//! Replaces known sensitive values in text with a placeholder.
//!
//! A [`Redactor`] is built once from the values to hide and then applied to any
//! string headed for a log, diagnostic or dry-run display. It matches values,
//! not names: deciding *which* values are sensitive is the caller's job.

use std::borrow::Cow;

pub use omni_constants::REDACTED_PLACEHOLDER;

/// Default minimum length for values added with
/// [`RedactorBuilder::bulk_value`].
pub const DEFAULT_BULK_MIN_LEN: usize = 4;

/// Replaces the userinfo (`user:password@`) of every URL-like token in `input`
/// with the placeholder, so credentials embedded in a URL (for example a git
/// remote) never reach logs or errors. Fixed rule: not part of any policy.
pub fn redact_url_userinfo(input: &str) -> Cow<'_, str> {
    let mut out: Option<String> = None;
    let mut rest = input;
    let mut consumed = 0;
    while let Some(i) = rest.find("://") {
        let auth_start = i + 3;
        let after = &rest[auth_start..];
        let auth_end = after
            .find(|c: char| matches!(c, '/' | '?' | '#') || c.is_whitespace())
            .unwrap_or(after.len());
        let authority = &after[..auth_end];
        if let Some(at) = authority.rfind('@') {
            let buf = out.get_or_insert_with(String::new);
            buf.push_str(&input[consumed..consumed + auth_start]);
            buf.push_str(REDACTED_PLACEHOLDER);
            buf.push('@');
            consumed += auth_start + at + 1;
            rest = &input[consumed..];
        } else {
            rest = &rest[auth_start..];
        }
    }
    match out {
        Some(mut buf) => {
            buf.push_str(&input[consumed..]);
            Cow::Owned(buf)
        }
        None => Cow::Borrowed(input),
    }
}

#[derive(Debug, Clone)]
pub struct Redactor {
    /// Sorted longest first, deduplicated, never empty.
    values: Vec<String>,
    placeholder: String,
}

impl Redactor {
    pub fn builder() -> RedactorBuilder {
        RedactorBuilder::default()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn placeholder(&self) -> &str {
        &self.placeholder
    }

    /// Replace every occurrence of each value with the placeholder.
    ///
    /// Longer values are replaced first so a short value cannot partially mask
    /// a longer one containing it. Works on the unsplit string, so values with
    /// whitespace match intact. Borrows when nothing matched.
    pub fn redact<'a>(&self, input: impl Into<Cow<'a, str>>) -> Cow<'a, str> {
        let mut text = input.into();
        for value in &self.values {
            if text.contains(value.as_str()) {
                text =
                    Cow::Owned(text.replace(value.as_str(), &self.placeholder));
            }
        }
        text
    }

    /// A redactor that hides the values of both `self` and `other`, using
    /// `self`'s placeholder.
    pub fn union(&self, other: &Redactor) -> Redactor {
        let mut values = self.values.clone();
        values.extend(other.values.iter().cloned());
        Redactor {
            values: normalize(values),
            placeholder: self.placeholder.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RedactorBuilder {
    placeholder: Option<String>,
    always: Vec<String>,
    bulk: Vec<String>,
    bulk_min_len: usize,
}

impl Default for RedactorBuilder {
    fn default() -> Self {
        Self {
            placeholder: None,
            always: Vec::new(),
            bulk: Vec::new(),
            bulk_min_len: DEFAULT_BULK_MIN_LEN,
        }
    }
}

impl RedactorBuilder {
    /// Text that replaces a value. Defaults to [`REDACTED_PLACEHOLDER`].
    pub fn placeholder(mut self, p: impl Into<String>) -> Self {
        self.placeholder = Some(p.into());
        self
    }

    /// Always redacted, however short. For names the user listed.
    pub fn value(mut self, v: impl Into<String>) -> Self {
        self.always.push(v.into());
        self
    }

    pub fn values<I, S>(mut self, v: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.always.extend(v.into_iter().map(Into::into));
        self
    }

    /// Redacted only when at least `bulk_min_len` characters long. For values
    /// redacted in bulk (`all: true`), where nobody chose the value.
    pub fn bulk_value(mut self, v: impl Into<String>) -> Self {
        self.bulk.push(v.into());
        self
    }

    pub fn bulk_values<I, S>(mut self, v: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.bulk.extend(v.into_iter().map(Into::into));
        self
    }

    /// Minimum length (in characters) for bulk values. Default 4.
    pub fn bulk_min_len(mut self, n: usize) -> Self {
        self.bulk_min_len = n;
        self
    }

    pub fn build(self) -> Redactor {
        let min = self.bulk_min_len;
        let mut values = self.always;
        values
            .extend(self.bulk.into_iter().filter(|v| v.chars().count() >= min));
        Redactor {
            values: normalize(values),
            placeholder: self
                .placeholder
                .unwrap_or_else(|| REDACTED_PLACEHOLDER.to_string()),
        }
    }
}

fn normalize(mut values: Vec<String>) -> Vec<String> {
    values.retain(|v| !v.is_empty());
    values.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    values.dedup();
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_values_and_borrows_when_unchanged() {
        let r = Redactor::builder().value("hunter2").build();
        assert_eq!(r.redact("pw=hunter2!"), "pw=<redacted>!");
        assert!(matches!(r.redact("nothing"), Cow::Borrowed(_)));
    }

    #[test]
    fn longest_first_and_overlap() {
        let r = Redactor::builder().values(["abc", "abcdef"]).build();
        assert_eq!(r.redact("abcdef abc"), "<redacted> <redacted>");
    }

    #[test]
    fn whitespace_values_match_intact() {
        let r = Redactor::builder().value("two words").build();
        assert_eq!(r.redact("x two words y"), "x <redacted> y");
    }

    #[test]
    fn empty_values_ignored() {
        let r = Redactor::builder().value("").build();
        assert!(r.is_empty());
        assert_eq!(r.redact("abc"), "abc");
    }

    #[test]
    fn value_is_redacted_however_short() {
        let r = Redactor::builder().value("1").build();
        assert_eq!(r.redact("a1"), "a<redacted>");
    }

    #[test]
    fn bulk_value_skipped_below_min_len_and_kept_at_it() {
        let r = Redactor::builder()
            .bulk_values(["1", "abc", "true", "abcd"])
            .build();
        assert_eq!(r.redact("1 abc true abcd"), "1 abc <redacted> <redacted>");
    }

    #[test]
    fn custom_bulk_min_len() {
        let r = Redactor::builder().bulk_min_len(1).bulk_value("1").build();
        assert_eq!(r.redact("a1"), "a<redacted>");
    }

    #[test]
    fn custom_placeholder() {
        let r = Redactor::builder().placeholder("***").value("s").build();
        assert_eq!(r.redact("s"), "***");
    }

    #[test]
    fn default_placeholder_is_shared_constant() {
        let r = Redactor::builder().build();
        assert_eq!(r.placeholder(), REDACTED_PLACEHOLDER);
    }

    #[test]
    fn url_userinfo_is_redacted() {
        assert_eq!(
            redact_url_userinfo("clone https://user:tok@github.com/a/b.git ok"),
            "clone https://<redacted>@github.com/a/b.git ok"
        );
        assert_eq!(
            redact_url_userinfo("ssh://git@host/x and https://a:b@h/p"),
            "ssh://<redacted>@host/x and https://<redacted>@h/p"
        );
        assert!(matches!(
            redact_url_userinfo("https://github.com/a/b@c"),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn union_combines() {
        let a = Redactor::builder().value("aaa").build();
        let b = Redactor::builder().value("bbb").build();
        assert_eq!(a.union(&b).redact("aaa bbb"), "<redacted> <redacted>");
    }
}
