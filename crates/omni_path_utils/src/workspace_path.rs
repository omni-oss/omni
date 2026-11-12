use std::fmt;
use std::path::Path;

/// Default maximum byte length of a [`WorkspacePath`].
pub const DEFAULT_MAX_WORKSPACE_PATH_LEN: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WorkspacePathError {
    #[error("workspace path is empty")]
    Empty,
    #[error("workspace path is {len} bytes, longer than the limit of {max}")]
    TooLong { len: usize, max: usize },
    #[error("workspace path contains a NUL byte")]
    Nul,
    #[error("workspace path contains a backslash: {0:?}")]
    Backslash(String),
    #[error("workspace path starts with a drive prefix: {0:?}")]
    DrivePrefix(String),
    #[error("workspace path is absolute: {0:?}")]
    Absolute(String),
    #[error("workspace path has an empty segment: {0:?}")]
    EmptySegment(String),
    #[error("workspace path has a `.` segment: {0:?}")]
    DotSegment(String),
    #[error("workspace path has a `..` segment: {0:?}")]
    DotDotSegment(String),
    #[error("path {path:?} is not inside the workspace root {root:?}")]
    OutsideRoot { path: String, root: String },
    #[error("path is not valid UTF-8")]
    NotUtf8,
}

/// Mirrors `WorkspacePath::parse` without lookaheads: `.`, or `/`-joined
/// segments that are not `.`/`..`, contain no `/`, `\` or NUL, and where the
/// first segment is not a drive prefix such as `C:`.
#[cfg(feature = "schemars")]
const WORKSPACE_PATH_PATTERN: &str = concat!(
    r"^(?:\.|",
    r"(?:[A-Za-z](?:[^/\\\x00:][^/\\\x00]*)?|[^/\\\x00.A-Za-z][^/\\\x00]*|\.[^/\\\x00.][^/\\\x00]*|\.\.[^/\\\x00]+)",
    r"(?:/(?:[^/\\\x00.][^/\\\x00]*|\.[^/\\\x00.][^/\\\x00]*|\.\.[^/\\\x00]+))*",
    r")$"
);

/// A validated, normalized path relative to the workspace root.
///
/// UTF-8, `/`-separated, no empty/`.`/`..` segments, no trailing `/`, no NUL,
/// no `\`. The root is exactly `.`. Casing is preserved and compared
/// byte-for-byte. Parsing never touches the filesystem, so symlinks are not
/// resolved; containment and symlink checks belong to access-time code.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WorkspacePath(String);

impl WorkspacePath {
    pub fn root() -> Self {
        Self(".".to_string())
    }

    pub fn is_root(&self) -> bool {
        self.0 == "."
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn segments(&self) -> impl Iterator<Item = &str> {
        self.0.split('/').filter(|_| !self.is_root())
    }

    /// Parses an already-normalized value. Rejects, never repairs.
    pub fn parse(value: &str) -> Result<Self, WorkspacePathError> {
        Self::parse_with_max_len(value, DEFAULT_MAX_WORKSPACE_PATH_LEN)
    }

    pub fn parse_with_max_len(
        value: &str,
        max_len: usize,
    ) -> Result<Self, WorkspacePathError> {
        if value.is_empty() {
            return Err(WorkspacePathError::Empty);
        }
        if value.len() > max_len {
            return Err(WorkspacePathError::TooLong {
                len: value.len(),
                max: max_len,
            });
        }
        if value.contains('\0') {
            return Err(WorkspacePathError::Nul);
        }
        if value.contains('\\') {
            return Err(WorkspacePathError::Backslash(value.to_string()));
        }
        let b = value.as_bytes();
        if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
            return Err(WorkspacePathError::DrivePrefix(value.to_string()));
        }
        if value == "." {
            return Ok(Self::root());
        }
        if value.starts_with('/') {
            return Err(WorkspacePathError::Absolute(value.to_string()));
        }
        for seg in value.split('/') {
            match seg {
                "" => {
                    return Err(WorkspacePathError::EmptySegment(
                        value.to_string(),
                    ));
                }
                "." => {
                    return Err(WorkspacePathError::DotSegment(
                        value.to_string(),
                    ));
                }
                ".." => {
                    return Err(WorkspacePathError::DotDotSegment(
                        value.to_string(),
                    ));
                }
                _ => {}
            }
        }
        Ok(Self(value.to_string()))
    }

    /// Converts an OS path to a workspace path after proving `path` is
    /// inside `root`. A relative `path` is taken as relative to `root`.
    /// Uses the separator rules of the current platform.
    pub fn from_os_path(
        root: &Path,
        path: &Path,
    ) -> Result<Self, WorkspacePathError> {
        let root = root.to_str().ok_or(WorkspacePathError::NotUtf8)?;
        let path = path.to_str().ok_or(WorkspacePathError::NotUtf8)?;
        Self::from_os_str(root, path, cfg!(windows))
    }

    /// Like [`Self::from_os_path`] but always applies Windows rules
    /// (`\` and `/` separators, drive, UNC and verbatim prefixes).
    pub fn from_windows_path(
        root: &str,
        path: &str,
    ) -> Result<Self, WorkspacePathError> {
        Self::from_os_str(root, path, true)
    }

    /// Like [`Self::from_os_path`] but always applies Unix rules, where a
    /// backslash is an ordinary name character and is rejected.
    pub fn from_unix_path(
        root: &str,
        path: &str,
    ) -> Result<Self, WorkspacePathError> {
        Self::from_os_str(root, path, false)
    }

    fn from_os_str(
        root: &str,
        path: &str,
        windows: bool,
    ) -> Result<Self, WorkspacePathError> {
        let outside = || WorkspacePathError::OutsideRoot {
            path: path.to_string(),
            root: root.to_string(),
        };
        let p = split_os(path, windows);
        let comps = if p.absolute {
            let r = split_os(root, windows);
            if !r.absolute || !r.prefix.eq_ignore_ascii_case(&p.prefix) {
                return Err(outside());
            }
            let eq = |a: &str, b: &str| {
                if windows {
                    a.to_lowercase() == b.to_lowercase()
                } else {
                    a == b
                }
            };
            let skip = |c: &&str| !c.is_empty() && *c != ".";
            let rc: Vec<&str> = r.comps.into_iter().filter(skip).collect();
            let pc: Vec<&str> = p.comps.into_iter().filter(skip).collect();
            if rc.len() > pc.len() || !rc.iter().zip(&pc).all(|(a, b)| eq(a, b))
            {
                return Err(outside());
            }
            pc[rc.len()..].to_vec()
        } else {
            p.comps
        };
        let mut out: Vec<&str> = Vec::new();
        for c in comps {
            match c {
                "" | "." => {}
                ".." => {
                    return Err(WorkspacePathError::DotDotSegment(
                        path.to_string(),
                    ));
                }
                c => out.push(c),
            }
        }
        if out.is_empty() {
            return Ok(Self::root());
        }
        Self::parse(&out.join("/"))
    }

    /// Appends a single normalized child segment.
    pub fn join(&self, segment: &str) -> Result<Self, WorkspacePathError> {
        if segment.contains('/') {
            return Err(WorkspacePathError::EmptySegment(segment.to_string()));
        }
        if segment == "." {
            return Err(WorkspacePathError::DotSegment(segment.to_string()));
        }
        if self.is_root() {
            Self::parse(segment)
        } else {
            Self::parse(&format!("{}/{}", self.0, segment))
        }
    }
}

struct OsParts<'a> {
    prefix: String,
    absolute: bool,
    comps: Vec<&'a str>,
}

fn split_os(s: &str, windows: bool) -> OsParts<'_> {
    if !windows {
        return OsParts {
            prefix: String::new(),
            absolute: s.starts_with('/'),
            comps: s.split('/').collect(),
        };
    }
    let (mut prefix, mut rest) = (String::new(), s);
    let is_sep = |c: char| c == '\\' || c == '/';
    let take_unc = |rest: &str| -> Option<(String, usize)> {
        let mut it = rest.split(is_sep);
        let server = it.next().filter(|x| !x.is_empty())?;
        let share = it.next().filter(|x| !x.is_empty())?;
        let consumed = server.len() + 1 + share.len();
        Some((format!("//{server}/{share}"), consumed))
    };
    let mut absolute = false;
    if let Some(r) = s.strip_prefix("\\\\?\\") {
        absolute = true;
        if r.as_bytes()
            .get(..4)
            .is_some_and(|b| b.eq_ignore_ascii_case(b"UNC\\"))
        {
            if let Some((pre, n)) = take_unc(&r[4..]) {
                prefix = pre;
                rest = &r[4 + n..];
            }
        } else if r.len() >= 2
            && r.as_bytes()[0].is_ascii_alphabetic()
            && r.as_bytes()[1] == b':'
        {
            prefix = r[..2].to_string();
            rest = &r[2..];
        } else {
            rest = r;
        }
    } else if s.starts_with("\\\\") || s.starts_with("//") {
        absolute = true;
        if let Some((pre, n)) = take_unc(&s[2..]) {
            prefix = pre;
            rest = &s[2 + n..];
        } else {
            rest = &s[2..];
        }
    } else if s.len() >= 2
        && s.as_bytes()[0].is_ascii_alphabetic()
        && s.as_bytes()[1] == b':'
    {
        prefix = s[..2].to_string();
        rest = &s[2..];
        absolute = rest.starts_with(is_sep);
        // `C:foo` is drive-relative, which cannot be proven inside a root.
        if !absolute {
            return OsParts {
                prefix,
                absolute: true,
                comps: vec!["..", ".."],
            };
        }
    } else if s.starts_with(is_sep) {
        absolute = true;
    }
    OsParts {
        prefix,
        absolute,
        comps: rest.split(is_sep).collect(),
    }
}

impl fmt::Display for WorkspacePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for WorkspacePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WorkspacePath({:?})", self.0)
    }
}

impl AsRef<str> for WorkspacePath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::str::FromStr for WorkspacePath {
    type Err = WorkspacePathError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl TryFrom<String> for WorkspacePath {
    type Error = WorkspacePathError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::parse(&s)
    }
}

impl From<WorkspacePath> for String {
    fn from(p: WorkspacePath) -> Self {
        p.0
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for WorkspacePath {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for WorkspacePath {
    fn deserialize<D: serde::Deserializer<'de>>(
        d: D,
    ) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(feature = "schemars")]
impl schemars::JsonSchema for WorkspacePath {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "WorkspacePath".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "minLength": 1,
            "maxLength": DEFAULT_MAX_WORKSPACE_PATH_LEN,
            "pattern": WORKSPACE_PATH_PATTERN,
            "description": "Normalized workspace-relative path: `/`-separated, no empty, `.` or `..` segments, no trailing `/`; the root is `.`.",
        })
    }
}

/// Identity of a project root: the workspace path of its directory.
/// Eligibility (whether the directory is a project) is not part of the type.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct ProjectRootId(WorkspacePath);

impl ProjectRootId {
    pub fn new(path: WorkspacePath) -> Self {
        Self(path)
    }

    pub fn path(&self) -> &WorkspacePath {
        &self.0
    }

    pub fn into_path(self) -> WorkspacePath {
        self.0
    }
}

impl From<WorkspacePath> for ProjectRootId {
    fn from(p: WorkspacePath) -> Self {
        Self(p)
    }
}

impl fmt::Display for ProjectRootId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(feature = "schemars")]
impl schemars::JsonSchema for ProjectRootId {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ProjectRootId".into()
    }

    fn json_schema(g: &mut schemars::SchemaGenerator) -> schemars::Schema {
        WorkspacePath::json_schema(g)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(s: &str) -> String {
        WorkspacePath::parse(s).unwrap().to_string()
    }

    #[test]
    fn parse_accepts_normalized() {
        assert_eq!(ok("."), ".");
        assert_eq!(ok("a"), "a");
        assert_eq!(ok("a/b/C"), "a/b/C");
        assert_eq!(ok("a b/.hidden/..x"), "a b/.hidden/..x");
        assert!(WorkspacePath::parse(".").unwrap().is_root());
    }

    #[test]
    fn parse_rejects_non_normalized() {
        use WorkspacePathError as E;
        assert_eq!(WorkspacePath::parse(""), Err(E::Empty));
        for bad in ["a//b", "a/", "/a", "a/b/"] {
            assert!(WorkspacePath::parse(bad).is_err(), "{bad}");
        }
        assert!(matches!(WorkspacePath::parse("./a"), Err(E::DotSegment(_))));
        assert!(matches!(
            WorkspacePath::parse("a/./b"),
            Err(E::DotSegment(_))
        ));
        assert!(matches!(
            WorkspacePath::parse(".."),
            Err(E::DotDotSegment(_))
        ));
        assert!(matches!(
            WorkspacePath::parse("a/../b"),
            Err(E::DotDotSegment(_))
        ));
        assert!(matches!(WorkspacePath::parse("a\\b"), Err(E::Backslash(_))));
        assert!(matches!(WorkspacePath::parse("a\0b"), Err(E::Nul)));
        assert!(matches!(WorkspacePath::parse("/a"), Err(E::Absolute(_))));
        assert!(WorkspacePath::parse("C:\\a").is_err());
    }

    #[test]
    fn parse_rejects_drive_prefix() {
        use WorkspacePathError as E;
        for bad in ["C:/Windows", "c:", "C:foo", "z:/a"] {
            assert!(
                matches!(WorkspacePath::parse(bad), Err(E::DrivePrefix(_))),
                "{bad}"
            );
        }
        assert_eq!(ok("ab:c"), "ab:c");
        assert_eq!(ok("dir/C:x"), "dir/C:x");
        assert_eq!(ok("1:/x"), "1:/x");
    }

    #[test]
    fn roots_are_normalized_before_containment() {
        let u =
            |r, p| WorkspacePath::from_unix_path(r, p).map(|p| p.to_string());
        assert_eq!(u("/ws/", "/ws/file").unwrap(), "file");
        assert_eq!(u("/ws//./", "/ws//a/./b").unwrap(), "a/b");
        assert_eq!(u("/", "/a/b").unwrap(), "a/b");
        assert_eq!(u("/", "/").unwrap(), ".");
        assert!(u("/ws/", "/ws/../x").is_err());
        assert!(u("/ws/", "/wsx").is_err());
        assert_eq!(win("C:\\", "C:\\a\\b").unwrap(), "a/b");
        assert_eq!(win("C:\\ws\\", "C:\\ws\\a").unwrap(), "a");
        assert_eq!(win("C:\\ws\\.\\", "C:\\ws\\a").unwrap(), "a");
        assert!(win("C:\\ws\\", "C:\\ws\\..\\x").is_err());
    }

    #[test]
    fn verbatim_non_ascii_does_not_panic() {
        for p in ["\\\\?\\abc\u{e9}", "\\\\?\\UN\u{e9}", "\\\\?\\\u{e9}\u{e9}"]
        {
            let _ = WorkspacePath::from_windows_path("C:\\ws", p);
            let _ = WorkspacePath::from_windows_path(p, "C:\\ws");
        }
    }

    #[test]
    fn length_bound_is_configurable() {
        assert!(WorkspacePath::parse_with_max_len("abcd", 4).is_ok());
        assert!(matches!(
            WorkspacePath::parse_with_max_len("abcde", 4),
            Err(WorkspacePathError::TooLong { len: 5, max: 4 })
        ));
        let long = "a".repeat(DEFAULT_MAX_WORKSPACE_PATH_LEN + 1);
        assert!(WorkspacePath::parse(&long).is_err());
    }

    #[test]
    fn case_variants_are_distinct() {
        assert_ne!(
            WorkspacePath::parse("Foo").unwrap(),
            WorkspacePath::parse("foo").unwrap()
        );
    }

    #[test]
    fn parser_does_not_resolve_symlink_like_names() {
        assert_eq!(
            ok("link/../x".replace("../", "..x/").as_str()),
            "link/..x/x"
        );
        assert!(WorkspacePath::parse("link/..").is_err());
    }

    fn win(root: &str, p: &str) -> Result<String, WorkspacePathError> {
        WorkspacePath::from_windows_path(root, p).map(|p| p.to_string())
    }

    #[test]
    fn windows_drive_and_separators() {
        assert_eq!(win("C:\\ws", "C:\\ws\\a\\b").unwrap(), "a/b");
        assert_eq!(win("C:\\ws", "C:/ws/a\\b").unwrap(), "a/b");
        assert_eq!(win("C:\\ws", "c:\\WS\\Pkg").unwrap(), "Pkg");
        assert_eq!(win("C:\\ws", "C:\\ws").unwrap(), ".");
        assert_eq!(win("C:\\ws", "C:\\ws\\").unwrap(), ".");
        assert_eq!(win("C:\\ws", "a\\b").unwrap(), "a/b");
        assert_eq!(win("C:\\ws", "C:\\ws\\a\\.\\b").unwrap(), "a/b");
    }

    #[test]
    fn windows_unc_and_verbatim() {
        assert_eq!(win("\\\\srv\\sh\\ws", "\\\\srv\\sh\\ws\\a").unwrap(), "a");
        assert_eq!(win("\\\\?\\C:\\ws", "\\\\?\\C:\\ws\\a\\b").unwrap(), "a/b");
        assert_eq!(win("C:\\ws", "\\\\?\\C:\\ws\\a").unwrap(), "a");
        assert_eq!(
            win("\\\\?\\UNC\\srv\\sh\\ws", "\\\\srv\\sh\\ws\\a").unwrap(),
            "a"
        );
        assert_eq!(
            win("\\\\?\\UNC\\srv\\sh\\ws", "\\\\?\\UNC\\srv\\sh\\ws\\x\\y")
                .unwrap(),
            "x/y"
        );
    }

    #[test]
    fn windows_rejects_outside_and_escapes() {
        use WorkspacePathError as E;
        assert!(matches!(
            win("C:\\ws", "D:\\ws\\a"),
            Err(E::OutsideRoot { .. })
        ));
        assert!(matches!(
            win("C:\\ws", "C:\\other\\a"),
            Err(E::OutsideRoot { .. })
        ));
        assert!(matches!(win("C:\\ws", "C:\\w"), Err(E::OutsideRoot { .. })));
        assert!(matches!(
            win("\\\\srv\\sh\\ws", "\\\\srv\\other\\ws\\a"),
            Err(E::OutsideRoot { .. })
        ));
        assert!(matches!(
            win("C:\\ws", "C:\\ws\\..\\x"),
            Err(E::DotDotSegment(_))
        ));
        assert!(win("C:\\ws", "C:a").is_err());
    }

    #[test]
    fn unix_conversion() {
        let u =
            |r, p| WorkspacePath::from_unix_path(r, p).map(|p| p.to_string());
        assert_eq!(u("/ws", "/ws/a//b/./c").unwrap(), "a/b/c");
        assert_eq!(u("/ws", "/ws").unwrap(), ".");
        assert_eq!(u("/ws", "a/b").unwrap(), "a/b");
        assert!(u("/ws", "/wsx/a").is_err());
        assert!(u("/ws", "/other").is_err());
        assert!(u("/ws", "/ws/../x").is_err());
        assert!(matches!(
            u("/ws", "/ws/a\\b"),
            Err(WorkspacePathError::Backslash(_))
        ));
        assert_ne!(u("/ws", "/ws/A").unwrap(), u("/ws", "/ws/a").unwrap());
    }

    #[test]
    fn from_os_path_uses_path_api() {
        let p =
            WorkspacePath::from_os_path(Path::new("/ws"), Path::new("/ws/x"));
        assert_eq!(p.unwrap().as_str(), "x");
    }

    #[test]
    fn idempotent() {
        let inputs = [
            "C:\\ws\\a\\b",
            "C:/ws/a/./b",
            "C:\\ws",
            "\\\\?\\C:\\ws\\a",
            "a\\b\\",
            "a",
        ];
        for i in inputs {
            if let Ok(p) = WorkspacePath::from_windows_path("C:\\ws", i) {
                assert_eq!(WorkspacePath::parse(p.as_str()).unwrap(), p);
                let again =
                    WorkspacePath::from_windows_path("C:\\ws", p.as_str())
                        .unwrap();
                assert_eq!(again, p);
            }
        }
    }

    #[test]
    fn join_and_segments() {
        let r = WorkspacePath::root();
        assert_eq!(r.segments().count(), 0);
        let a = r.join("a").unwrap().join("B").unwrap();
        assert_eq!(a.as_str(), "a/B");
        assert_eq!(a.segments().collect::<Vec<_>>(), ["a", "B"]);
        for bad in ["", ".", "..", "a/b", "a\\b"] {
            assert!(r.join(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn join_is_consistent_with_parse() {
        let dir = WorkspacePath::parse("dir").unwrap();
        assert_eq!(
            dir.join("C:x").unwrap(),
            WorkspacePath::parse("dir/C:x").unwrap()
        );
        assert!(matches!(
            WorkspacePath::root().join("C:x"),
            Err(WorkspacePathError::DrivePrefix(_))
        ));
        assert!(dir.join("").is_err());
    }

    #[test]
    fn project_root_id_wraps() {
        let id = ProjectRootId::from(WorkspacePath::parse("apps/x").unwrap());
        assert_eq!(id.to_string(), "apps/x");
        assert_eq!(id.path().as_str(), "apps/x");
    }
}
