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

/// Split a `host[:port]` pattern into its host and optional port parts. The
/// port is recognized only when it is `*` or all-digits, so `example.com`
/// (no port) and IPv6-ish hosts are not mis-split.
pub(crate) fn split_host_port(pattern: &str) -> (&str, Option<&str>) {
    if let Some((h, p)) = pattern.rsplit_once(':') {
        let looks_like_port = p == "*"
            || (!p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
        if looks_like_port {
            return (h, Some(p));
        }
    }
    (pattern, None)
}
