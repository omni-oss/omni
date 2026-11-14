use std::fmt;

use crate::error::Error;

/// A validated, slash-separated location inside a storage backend (for
/// example `tenant/org/ws/env`). Backends only ever receive this type, so a
/// path that escapes its container cannot reach them.
///
/// Segments must be non-empty and must not be `.` or `..`; the whole value
/// has no leading or trailing `/`, no `\`, and no control characters.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Container(String);

impl Container {
    pub fn parse(raw: impl Into<String>) -> Result<Self, Error> {
        let raw = raw.into();
        validate(&raw)?;
        Ok(Self(raw))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The prefix that covers exactly this container (always ends in `/`).
    pub fn prefix(&self) -> ContainerPrefix {
        ContainerPrefix(format!("{}/", self.0))
    }
}

impl AsRef<str> for Container {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Container {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A validated container-level prefix used for scans and prefix deletes. It
/// always ends in `/`, so a prefix for `ci` can never match `ci2`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ContainerPrefix(String);

impl ContainerPrefix {
    pub fn parse(raw: impl Into<String>) -> Result<Self, Error> {
        let raw = raw.into();
        let body = raw.strip_suffix('/').ok_or_else(|| {
            Error::custom(eyre::eyre!("container prefix must end in '/'"))
        })?;
        validate(body)?;
        Ok(Self(raw))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for ContainerPrefix {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<&Container> for ContainerPrefix {
    fn from(c: &Container) -> Self {
        c.prefix()
    }
}

fn validate(raw: &str) -> Result<(), Error> {
    let bad = |why: &str| {
        Err(Error::custom(eyre::eyre!(
            "invalid container {raw:?}: {why}"
        )))
    };
    if raw.is_empty() {
        return bad("empty");
    }
    if raw.contains('\\') || raw.chars().any(|c| c.is_control()) {
        return bad("backslash or control character");
    }
    for seg in raw.split('/') {
        if seg.is_empty() {
            return bad("empty segment");
        }
        if seg == "." || seg == ".." {
            return bad("dot segment");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_paths() {
        assert_eq!(Container::parse("t/o/w/e").unwrap().as_str(), "t/o/w/e");
        assert_eq!(Container::parse("default").unwrap().as_str(), "default");
    }

    #[test]
    fn rejects_unsafe_paths() {
        for bad in [
            "", "/a", "a/", "a//b", "a/../b", "..", ".", "a/./b", "a\\b",
            "a\0b", "a\nb",
        ] {
            assert!(Container::parse(bad).is_err(), "{bad:?} must be rejected");
        }
    }

    #[test]
    fn prefix_ends_in_slash_and_never_matches_a_sibling() {
        let ci = Container::parse("ns/ci").unwrap().prefix();
        assert_eq!(ci.as_str(), "ns/ci/");
        assert!(!"ns/ci2/key".starts_with(ci.as_str()));
        assert!("ns/ci/key".starts_with(ci.as_str()));
    }

    #[test]
    fn prefix_requires_trailing_slash_and_valid_body() {
        assert!(ContainerPrefix::parse("a/b").is_err());
        assert!(ContainerPrefix::parse("a/../").is_err());
        assert!(ContainerPrefix::parse("a/b/").is_ok());
    }
}
