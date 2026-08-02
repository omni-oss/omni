//! Shared, runtime-agnostic helpers for lowering *resolved* policy patterns into
//! the concrete forms individual pre-spawn backends need.
//!
//! Both the Deno and Node backends translate the same neutral
//! [`RequiredCapabilities`](omni_capabilities::RequiredCapabilities) into their
//! own flag vocabulary, and they agree on what a filesystem glob and a
//! `host:port` selector *mean*. That agreement lives here so the per-runtime
//! modules only encode the differences (flag names, whether deny-lists exist,
//! whether wildcards are supported).

/// Characters that make a string a glob rather than a literal path/name.
fn is_glob_meta(c: char) -> bool {
    matches!(c, '*' | '?' | '[' | ']' | '{' | '}')
}

pub(crate) fn has_glob(s: &str) -> bool {
    s.chars().any(is_glob_meta)
}

/// The confinement scope a resolved filesystem glob describes, reduced to
/// something a path/prefix-based permission model can express faithfully.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FsScope {
    /// An entire subtree rooted at this path (from a `<prefix>/**` pattern).
    Subtree(String),
    /// A single exact file or directory (no globbing).
    Exact(String),
}

/// Classify a resolved fs glob for a prefix-based permission model, rejecting
/// anything that cannot be represented without changing its meaning.
///
/// Deliberately conservative: a whole-filesystem pattern (`**`, `*`, `/**`) is
/// rejected rather than lowered to "allow everything", because granting
/// unconfined filesystem access defeats the purpose of the sandbox — callers
/// must name an explicit root instead. Mid-path globs and extension filters
/// (`src/*.rs`) are rejected because a prefix grant would silently widen them.
pub(crate) fn classify_fs_glob(glob: &str) -> Result<FsScope, String> {
    if let Some(prefix) = glob.strip_suffix("/**") {
        if prefix.is_empty() {
            return Err(whole_fs(glob));
        }
        if has_glob(prefix) {
            return Err(format!(
                "only a trailing `/**` can be lowered to a path prefix, but \
                 `{glob}` contains globs before it"
            ));
        }
        return Ok(FsScope::Subtree(prefix.to_string()));
    }

    if glob == "**" || glob == "*" {
        return Err(whole_fs(glob));
    }

    if has_glob(glob) {
        return Err(format!(
            "path-prefix permissions cannot represent the glob `{glob}` \
             without widening access; grant an explicit directory, or use an \
             in-process broker for precise globs"
        ));
    }

    Ok(FsScope::Exact(glob.to_string()))
}

fn whole_fs(glob: &str) -> String {
    format!(
        "`{glob}` matches the entire filesystem; grant an explicit root (e.g. \
         `@workspace/**`) instead of unconfined access"
    )
}

/// Reject a resolved flag value that cannot be placed into a comma-joined
/// `--flag=a,b,c` launch-flag list without changing its meaning.
///
/// Backends emit their allow/deny values as a single `--flag=<v1>,<v2>,…`
/// argument, so a value containing a `,` would silently inject *extra* list
/// entries (a path or host that embeds a comma could smuggle in an allowance
/// the policy never granted). Control characters and newlines are refused for
/// the same defense-in-depth reason (they can confuse the runtime's flag parser
/// or downstream logging), and `=` is refused conservatively since it is the
/// flag-name/value delimiter. Such a value is *unrepresentable as a launch
/// flag* — the caller must turn it into a [`Gap`](crate::Gap) (resolved by the
/// in-process broker, which matches on the true string) rather than widen
/// access by emitting it verbatim.
pub(crate) fn validate_flag_value(value: &str) -> Result<(), String> {
    if let Some(bad) = value
        .chars()
        .find(|&c| c == ',' || c == '=' || c.is_control())
    {
        return Err(format!(
            "value `{value}` contains the character {bad:?}, which cannot be \
             carried in a comma-separated launch-flag list without changing \
             its meaning (resolved precisely by the in-process broker instead)"
        ));
    }
    Ok(())
}

/// Whether `p` is a port selector: a bare `*` (any port) or all ASCII digits.
fn is_port(p: &str) -> bool {
    p == "*" || (!p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Split a `host[:port]` pattern into its host and optional port parts.
///
/// Handles IPv6 in both forms so a colon inside the address is never mistaken
/// for the port delimiter:
/// * **Bracketed** (`[::1]`, `[::1]:443`) — the port, if present, follows the
///   closing `]`; colons inside the brackets are part of the host.
/// * **Bare** (`::1`, `fe80::1`) — the tail after the final colon is treated as
///   a port only when the host part has no remaining colon (i.e. the split
///   colon is the sole one), so a bracket-less IPv6 literal stays intact.
pub(crate) fn split_host_port(pattern: &str) -> (&str, Option<&str>) {
    if let Some(rest) = pattern.strip_prefix('[') {
        // Bracketed IPv6: the host is everything up to and including `]`.
        let Some(close) = rest.find(']') else {
            return (pattern, None); // malformed — treat as an opaque host
        };
        let host = &pattern[..close + 2];
        let after = &pattern[close + 2..];
        if let Some(port) = after.strip_prefix(':')
            && is_port(port)
        {
            return (host, Some(port));
        }
        return (host, None);
    }

    if let Some((h, p)) = pattern.rsplit_once(':')
        && !h.contains(':')
        && is_port(p)
    {
        return (h, Some(p));
    }
    (pattern, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_value_rejects_comma_and_equals_and_control() {
        assert!(validate_flag_value("/repo/src").is_ok());
        assert!(validate_flag_value("example.com:443").is_ok());
        // A comma would inject an extra allow entry.
        assert!(validate_flag_value("/a,/etc/passwd").is_err());
        assert!(validate_flag_value("a=b").is_err());
        assert!(validate_flag_value("line\nbreak").is_err());
        assert!(validate_flag_value("nul\0byte").is_err());
    }

    #[test]
    fn host_port_split_basic() {
        assert_eq!(split_host_port("example.com"), ("example.com", None));
        assert_eq!(
            split_host_port("example.com:443"),
            ("example.com", Some("443"))
        );
        assert_eq!(
            split_host_port("example.com:*"),
            ("example.com", Some("*"))
        );
    }

    #[test]
    fn host_port_split_ipv6() {
        // Bare IPv6 must not be split on an internal colon.
        assert_eq!(split_host_port("::1"), ("::1", None));
        assert_eq!(split_host_port("fe80::1"), ("fe80::1", None));
        // Bracketed IPv6, with and without a port.
        assert_eq!(split_host_port("[::1]"), ("[::1]", None));
        assert_eq!(split_host_port("[::1]:443"), ("[::1]", Some("443")));
        assert_eq!(
            split_host_port("[fe80::1]:8080"),
            ("[fe80::1]", Some("8080"))
        );
    }
}
