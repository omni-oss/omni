//! Request matching: turning a concrete operation ([`Request`]) plus the
//! ambient [`PathRoots`] into a yes/no against a single [`CapabilityRule`].
//!
//! Matching is purely lexical and deterministic — it performs no filesystem or
//! network I/O. Symlink resolution, real-path canonicalization, and DNS are
//! enforcement-layer concerns, not policy concerns.
//!
//! Filesystem patterns use omni's rooted-path convention via
//! [`omni_types::OmniPath`]: a pattern may be prefixed with `@<root>/` (e.g.
//! `@workspace/**`, `@project/src/**`) which is resolved against [`PathRoots`],
//! or it may be a plain (relative/absolute) glob. The tail after the root is a
//! glob, matched with `/`-aware semantics.
//!
//! [`PathRoots`] is generic over the root enum (`TRoot: OmniPathRoot`), exactly
//! like [`omni_types::OmniPath`], defaulting to [`omni_types::Root`]. A
//! subsystem that needs extra roots can supply its own enum implementing
//! [`OmniPathRoot`] without changing this module.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use omni_types::{OmniPath, OmniPathRoot, Root};
use path_clean::PathClean;

use crate::{CapabilityDomain, CapabilityRule};

/// Resolves omni path roots (e.g. `@workspace`, `@project`) used in filesystem
/// patterns. Keeping roots abstract is what makes the same config portable
/// across operating systems.
///
/// Generic over the root enum `TRoot` (defaults to [`omni_types::Root`]),
/// mirroring [`omni_types::OmniPath`]. A root that is not registered here causes
/// any pattern referencing it to match nothing (fail-closed).
#[derive(Debug, Clone)]
pub struct PathRoots<TRoot: OmniPathRoot = Root> {
    // A small association list: roots are few, and `OmniPathRoot` guarantees
    // `PartialEq` + `Copy` but not `Ord`/`Hash`, so a `Vec` avoids extra bounds.
    bases: Vec<(TRoot, PathBuf)>,
}

impl<TRoot: OmniPathRoot> Default for PathRoots<TRoot> {
    fn default() -> Self {
        Self { bases: Vec::new() }
    }
}

impl<TRoot: OmniPathRoot> PathRoots<TRoot> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `@root` → `base` (builder form).
    pub fn with(mut self, root: TRoot, base: impl Into<PathBuf>) -> Self {
        self.set(root, base);
        self
    }

    pub fn set(&mut self, root: TRoot, base: impl Into<PathBuf>) {
        let base = base.into();
        if let Some(slot) = self.bases.iter_mut().find(|(r, _)| *r == root) {
            slot.1 = base;
        } else {
            self.bases.push((root, base));
        }
    }

    /// Return a copy with every root **base** rewritten by `f`.
    ///
    /// This is the seam the enforcement layer uses to *canonicalize* root bases
    /// (resolving any symlinks in them) before authorization. Matching stays
    /// purely lexical and I/O-free — the caller supplies the resolving function
    /// (e.g. `std::fs::canonicalize`), so no filesystem access leaks into this
    /// crate. Canonical roots are what let a symlink-resolved *real* path be
    /// re-authorized without spuriously escaping a root that itself lives under
    /// a symlink.
    pub fn map_bases(mut self, mut f: impl FnMut(PathBuf) -> PathBuf) -> Self {
        for (_, base) in &mut self.bases {
            *base = f(std::mem::take(base));
        }
        self
    }

    fn base(&self, root: TRoot) -> Option<&Path> {
        self.bases
            .iter()
            .find(|(r, _)| *r == root)
            .map(|(_, p)| p.as_path())
    }

    /// Resolves a filesystem pattern into a concrete glob string.
    ///
    /// * `@root/tail` → `<base>/tail` when the root is registered, else `None`
    ///   (the pattern cannot match anything).
    /// * a plain glob is returned as-is.
    ///
    /// The resolution is lexical (no filesystem access, no CWD): the root base
    /// is joined with the tail and cleaned, never made absolute against the
    /// process working directory.
    ///
    /// Enforcement backends reuse this to lower policy patterns into the
    /// concrete, platform-neutral paths they need (e.g. Deno `--allow-read`
    /// prefixes, WASI preopens).
    pub fn resolve_pattern(&self, pattern: &str) -> Option<String> {
        // A bare `@root` with no `/tail` is malformed; reject it explicitly
        // (also avoids `OmniPath::from_str` panicking on the missing tail).
        if pattern.starts_with('@') && !pattern.contains('/') {
            return None;
        }

        // Reuse OmniPath's `@root/tail` parsing + `TRoot`'s name parsing.
        let parsed = OmniPath::<TRoot>::from_str(pattern).ok()?;
        match parsed.root() {
            Some(root) => {
                let base = self.base(root)?;
                let joined = base.join(parsed.unresolved_path());
                Some(to_forward(&joined.clean().to_string_lossy()))
            }
            None => Some(to_forward(pattern)),
        }
    }
}

/// A concrete operation to authorize.
#[derive(Debug, Clone, Copy)]
pub enum Request<'a> {
    /// A filesystem access; `write = true` selects
    /// [`FsWrite`](CapabilityDomain::FsWrite), otherwise
    /// [`FsRead`](CapabilityDomain::FsRead).
    Fs { write: bool, path: &'a Path },
    /// An outbound network connection.
    Net { host: &'a str, port: u16 },
    /// Reading an environment variable.
    Env { name: &'a str },
    /// Spawning a child process.
    Process { program: &'a str },
}

impl Request<'_> {
    pub fn domain(&self) -> CapabilityDomain {
        match self {
            Request::Fs { write: false, .. } => CapabilityDomain::FsRead,
            Request::Fs { write: true, .. } => CapabilityDomain::FsWrite,
            Request::Net { .. } => CapabilityDomain::Net,
            Request::Env { .. } => CapabilityDomain::Env,
            Request::Process { .. } => CapabilityDomain::Process,
        }
    }

    /// A human-readable rendering of the requested value, for diagnostics.
    pub fn value_string(&self) -> String {
        match self {
            Request::Fs { path, .. } => path.display().to_string(),
            Request::Net { host, port } => format!("{host}:{port}"),
            Request::Env { name } => (*name).to_string(),
            Request::Process { program } => (*program).to_string(),
        }
    }
}

/// Returns `true` if `rule` matches `req` (same domain and at least one pattern
/// matches). Filesystem patterns are resolved through `roots`; `net` / `env` /
/// `process` patterns are matched verbatim (they are not paths).
pub fn rule_matches<R: OmniPathRoot>(
    rule: &CapabilityRule,
    req: &Request,
    roots: &PathRoots<R>,
) -> bool {
    if rule.domain != req.domain() {
        return false;
    }
    match req {
        Request::Fs { path, .. } => rule.patterns.iter().any(|p| {
            roots
                .resolve_pattern(p)
                .is_some_and(|glob| path_glob_matches(&glob, path))
        }),
        Request::Net { host, port } => rule
            .patterns
            .iter()
            .any(|p| host_port_matches(p, host, *port)),
        Request::Env { name } => {
            rule.patterns.iter().any(|p| glob_str_matches(p, name))
        }
        Request::Process { program } => {
            rule.patterns.iter().any(|p| glob_str_matches(p, program))
        }
    }
}

// ── glob helpers ───────────────────────────────────────────────────────────

fn to_forward(s: &str) -> String {
    s.replace('\\', "/")
}

fn compile(
    pattern: &str,
    literal_separator: bool,
) -> Option<globset::GlobMatcher> {
    globset::GlobBuilder::new(pattern)
        .literal_separator(literal_separator)
        .build()
        .ok()
        .map(|g| g.compile_matcher())
}

/// Path glob with `/`-aware semantics: `*` does not cross directory
/// separators, `**` does.
fn path_glob_matches(glob: &str, path: &Path) -> bool {
    let target = to_forward(&path.clean().to_string_lossy());
    compile(glob, true).is_some_and(|m| m.is_match(&target))
}

/// Plain glob (no separator awareness), for hosts/env names/program names.
fn glob_str_matches(pattern: &str, value: &str) -> bool {
    compile(pattern, false).is_some_and(|m| m.is_match(value))
}

/// Matches a `host:port` request against a `host[:port]` pattern where the host
/// part is a glob and the port is exact, `*`, or omitted (any).
fn host_port_matches(pattern: &str, host: &str, port: u16) -> bool {
    let (p_host, p_port) = split_host_port(pattern);
    let host_ok = glob_str_matches(p_host, host);
    let port_ok = match p_port {
        None => true,
        Some("*") => true,
        Some(p) => p.parse::<u16>().is_ok_and(|n| n == port),
    };
    host_ok && port_ok
}

fn split_host_port(pattern: &str) -> (&str, Option<&str>) {
    if let Some((h, p)) = pattern.rsplit_once(':') {
        let looks_like_port = p == "*"
            || (!p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
        if looks_like_port {
            return (h, Some(p));
        }
    }
    (pattern, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots() -> PathRoots {
        PathRoots::new()
            .with(Root::Workspace, "/repo")
            .with(Root::Project, "/repo/pkg")
    }

    #[test]
    fn resolves_workspace_root_prefix() {
        let g = roots().resolve_pattern("@workspace/**").unwrap();
        assert_eq!(g, "/repo/**");
    }

    #[test]
    fn resolves_project_root_prefix() {
        let g = roots().resolve_pattern("@project/src/**").unwrap();
        assert_eq!(g, "/repo/pkg/src/**");
    }

    #[test]
    fn plain_glob_passes_through() {
        let g = roots().resolve_pattern("**/.git/**").unwrap();
        assert_eq!(g, "**/.git/**");
    }

    #[test]
    fn unregistered_root_resolves_to_none() {
        // `@project` not registered → pattern matches nothing (fail-closed).
        let only_ws = PathRoots::new().with(Root::Workspace, "/repo");
        assert!(only_ws.resolve_pattern("@project/**").is_none());
    }

    #[test]
    fn invalid_root_resolves_to_none() {
        assert!(roots().resolve_pattern("@bogus/**").is_none());
    }

    #[test]
    fn bare_root_without_tail_resolves_to_none() {
        assert!(roots().resolve_pattern("@workspace").is_none());
    }

    #[test]
    fn map_bases_rewrites_every_root_base() {
        // The enforcement layer uses this to canonicalize root bases; here we
        // just prove every base is passed through the closure and the mapping
        // is reflected in subsequent resolution.
        let mapped = roots().map_bases(|p| {
            PathBuf::from(p.to_string_lossy().replace("/repo", "/real"))
        });
        assert_eq!(
            mapped.resolve_pattern("@workspace/**").unwrap(),
            "/real/**"
        );
        assert_eq!(
            mapped.resolve_pattern("@project/src/**").unwrap(),
            "/real/pkg/src/**"
        );
    }

    #[test]
    fn rooted_glob_matches_nested_path() {
        let rule = CapabilityRule {
            access: crate::Access::Allow,
            domain: CapabilityDomain::FsRead,
            patterns: vec!["@workspace/**".into()],
            on_unenforceable: None,
        };
        assert!(rule_matches(
            &rule,
            &Request::Fs {
                write: false,
                path: Path::new("/repo/src/a.rs")
            },
            &roots(),
        ));
        assert!(!rule_matches(
            &rule,
            &Request::Fs {
                write: false,
                path: Path::new("/etc/passwd")
            },
            &roots(),
        ));
    }

    #[test]
    fn host_port_wildcards() {
        assert!(host_port_matches(
            "*.npmjs.org:443",
            "registry.npmjs.org",
            443
        ));
        assert!(host_port_matches("example.com:*", "example.com", 8080));
        assert!(host_port_matches("example.com", "example.com", 22));
        assert!(!host_port_matches("example.com:443", "example.com", 80));
    }
}
