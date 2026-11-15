use std::{
    borrow::Cow,
    fmt::Display,
    path::{Path as StdPath, PathBuf},
    str::FromStr,
};

use enum_map::{Enum, EnumMap};
use strum::{Display, EnumDiscriminants, IntoDiscriminant as _};

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    enum_map::Enum,
    Display,
    strum::VariantArray,
    strum::EnumString,
)]
#[strum(serialize_all = "kebab-case")]
pub enum Root {
    #[strum(serialize = "workspace")]
    Workspace,
    #[strum(serialize = "project")]
    Project,
}

pub type RootMap<'a> = EnumMap<Root, &'a StdPath>;

pub trait OmniPathRoot:
    Copy + Clone + Display + PartialEq + FromStr + strum::VariantArray + Enum
{
}

impl<
    T: Copy + Clone + Display + PartialEq + FromStr + strum::VariantArray + Enum,
> OmniPathRoot for T
{
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OmniPath<TRoot: OmniPathRoot = Root> {
    path: PathBuf,
    root: Option<TRoot>,
}

impl<TRoot: OmniPathRoot> Default for OmniPath<TRoot> {
    fn default() -> Self {
        Self::new("")
    }
}

impl<TRoot: OmniPathRoot> From<&OmniPath<TRoot>> for OmniPath<TRoot> {
    fn from(value: &OmniPath<TRoot>) -> Self {
        value.clone()
    }
}

#[cfg(feature = "schemars")]
impl<T: OmniPathRoot> schemars::JsonSchema for OmniPath<T> {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "OmniPath".into()
    }

    fn json_schema(
        generator: &mut schemars::SchemaGenerator,
    ) -> schemars::Schema {
        let mut schema = String::json_schema(generator);
        schema.insert(
            "description".to_string(),
            "A path optionally anchored to a named root with `@root/...`. To use \
             a literal leading `@` (e.g. an npm scope like `@myorg/pkg`), escape \
             it as `\\@myorg/pkg`."
                .into(),
        );
        schema
    }
}

impl<TRoot: OmniPathRoot> OmniPath<TRoot> {
    #[inline(always)]
    pub fn new_rooted(path: impl Into<PathBuf>, root: TRoot) -> Self {
        Self {
            path: path.into(),
            root: Some(root),
        }
    }

    #[inline(always)]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            root: None,
        }
    }

    #[inline(always)]
    pub fn root(&self) -> Option<TRoot> {
        self.root
    }

    #[inline(always)]
    pub fn is_rooted(&self, root: TRoot) -> bool {
        self.root.is_some() && self.root == Some(root)
    }

    #[inline(always)]
    pub fn is_any_rooted(&self) -> bool {
        self.root.is_some()
    }

    #[inline(always)]
    pub fn path(&self) -> Result<&StdPath, OmniPathError> {
        if self.root.is_some() {
            Err(OmniPathErrorInner::NotResolved.into())
        } else {
            Ok(&self.path)
        }
    }

    #[inline(always)]
    pub fn unresolved_path(&self) -> &StdPath {
        &self.path
    }

    /// True when this path is anchored to a named root and can only descend
    /// from it: non-empty, relative, and made of plain names (a `.` is
    /// ignored). Used to check untrusted paths, such as those inside a
    /// downloaded cache entry, before they are joined onto a root.
    pub fn is_confined(&self) -> bool {
        self.root.is_some() && is_confined_relative(&self.path)
    }

    /// Resolves the path relative to the given base
    #[inline(always)]
    pub fn resolve<'a>(
        &'a self,
        base: &EnumMap<TRoot, &StdPath>,
    ) -> Cow<'a, StdPath> {
        if let Some(root) = self.root {
            Cow::Owned(
                std::path::absolute(base[root].join(&self.path))
                    .expect("it should be absolute"),
            )
        } else {
            Cow::Borrowed(&self.path)
        }
    }

    #[inline(always)]
    pub fn resolve_in_place(&mut self, base: &EnumMap<TRoot, &StdPath>) {
        if let Some(root) = self.root {
            *self = Self {
                path: std::path::absolute(base[root].join(&self.path))
                    .expect("it should be absolute"),
                root: None,
            };
        }
    }
}

impl OmniPath<Root> {
    /// True when this path is anchored to a root and, once `..` segments are
    /// resolved lexically, still lies strictly inside the workspace root. A
    /// sibling project directory (`@project/../shared/x`) is allowed; leaving
    /// the workspace, an absolute or drive-prefixed path, an unrooted path, an
    /// empty path and unsafe names (see [`is_confined_relative`]) are not.
    ///
    /// Lexical only: it does not look at symlinks on disk.
    pub fn stays_within_workspace(&self, roots: &RootMap) -> bool {
        use std::path::Component;
        let Some(root) = self.root else {
            return false;
        };
        // An empty or dot-only path names no file.
        if !self
            .path
            .components()
            .any(|c| matches!(c, Component::Normal(_)))
        {
            return false;
        }
        let mut resolved: Vec<std::ffi::OsString> = Vec::new();
        let mut push_all = |path: &StdPath, check_names: bool| -> bool {
            for component in path.components() {
                match component {
                    Component::Prefix(_) | Component::RootDir => {
                        if check_names {
                            return false;
                        }
                        resolved.clear();
                    }
                    Component::CurDir => {}
                    Component::ParentDir => {
                        if resolved.pop().is_none() {
                            return false;
                        }
                    }
                    Component::Normal(name) => {
                        if check_names {
                            let text = name.to_string_lossy();
                            if text.chars().any(char::is_control)
                                || (cfg!(windows)
                                    && is_windows_unsafe_name(&text))
                            {
                                return false;
                            }
                        }
                        resolved.push(name.to_os_string());
                    }
                }
            }
            true
        };
        // The roots are trusted absolute paths; `Prefix`/`RootDir` in them
        // restart the accumulation.
        if !push_all(roots[root], false) || !push_all(&self.path, true) {
            return false;
        }
        let mut workspace: Vec<std::ffi::OsString> = Vec::new();
        for component in roots[Root::Workspace].components() {
            match component {
                Component::Prefix(_) | Component::RootDir => workspace.clear(),
                Component::CurDir => {}
                Component::ParentDir => {
                    workspace.pop();
                }
                Component::Normal(name) => workspace.push(name.to_os_string()),
            }
        }
        resolved.len() > workspace.len() && resolved.starts_with(&workspace)
    }
}

/// True for a non-empty relative path whose components are all plain names (or
/// `.`). Rejects `..`, a root directory, and a Windows drive or UNC prefix, each
/// of which would let `base.join(path)` leave `base` (a rooted or prefixed
/// argument replaces the base outright). A name with a control character is
/// rejected on every platform; on Windows a name that Win32 would normalize or
/// treat specially is rejected too (see [`is_windows_unsafe_name`]).
pub fn is_confined_relative(path: &StdPath) -> bool {
    use std::path::Component;
    let mut names = 0usize;
    for component in path.components() {
        match component {
            Component::Normal(name) => {
                let name = name.to_string_lossy();
                if name.chars().any(char::is_control)
                    || (cfg!(windows) && is_windows_unsafe_name(&name))
                {
                    return false;
                }
                names += 1;
            }
            Component::CurDir => {}
            Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_) => {
                return false;
            }
        }
    }
    names > 0
}

/// True for a file name that Windows treats as something other than the plain
/// file it looks like. `std::path::absolute` calls `GetFullPathNameW`, which
/// strips trailing dots and spaces, so `".. "` or `"..."` is not a harmless name;
/// a `:` selects an alternate data stream or a drive; some characters are
/// wildcards; and a device name such as `CON` or `NUL.txt` opens a device.
///
/// Pure string logic, so it is tested on every platform.
pub fn is_windows_unsafe_name(name: &str) -> bool {
    if name.ends_with('.') || name.ends_with(' ') {
        return true;
    }
    if name
        .chars()
        .any(|c| matches!(c, ':' | '<' | '>' | '"' | '|' | '?' | '*'))
    {
        return true;
    }
    // A device name is reserved with or without an extension.
    let stem = name.split('.').next().unwrap_or(name).trim_end_matches(' ');
    let stem = stem.to_ascii_uppercase();
    matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|prefix| {
        stem.strip_prefix(prefix).is_some_and(|rest| {
            rest.len() == 1 && rest.as_bytes()[0].is_ascii_digit()
        })
    })
}

#[derive(Debug, thiserror::Error)]
#[error("{inner}")]
pub struct OmniPathError {
    kind: OmniPathErrorKind,
    #[source]
    inner: OmniPathErrorInner,
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(name(OmniPathErrorKind), vis(pub))]
enum OmniPathErrorInner {
    #[error("path is not resolved")]
    NotResolved,
}

impl<T: Into<OmniPathErrorInner>> From<T> for OmniPathError {
    fn from(value: T) -> Self {
        let inner = value.into();
        let kind = inner.discriminant();
        Self { inner, kind }
    }
}

impl From<&StdPath> for OmniPath {
    fn from(path: &StdPath) -> Self {
        Self::new(path)
    }
}

impl From<PathBuf> for OmniPath {
    fn from(path: PathBuf) -> Self {
        Self::new(path)
    }
}

impl<TRoot: OmniPathRoot> FromStr for OmniPath<TRoot> {
    type Err = <TRoot as FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // A leading `\@` escapes to a literal leading `@`; a leading `\` not
        // followed by `@` is an ordinary literal backslash. Nothing else is
        // special.
        if let Some(rest) = s.strip_prefix("\\@") {
            return Ok(Self::new(format!("@{rest}")));
        }

        if let Some(after_at) = s.strip_prefix('@') {
            let (root, path) = match after_at.split_once('/') {
                Some((root, path)) => (root, path),
                None => (after_at, ""),
            };

            return Ok(Self::new_rooted(path, TRoot::from_str(root)?));
        }

        Ok(Self::new(s))
    }
}

impl<TRoot: OmniPathRoot> Display for OmniPath<TRoot> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(root) = self.root {
            write!(f, "@{}/{}", root, self.path.display())
        } else {
            let rendered = self.path.display().to_string();
            // Symmetric with `from_str`: an unrooted path whose first segment
            // begins with `@` is emitted as `\@...` so it round-trips instead
            // of being re-parsed as a root token.
            if rendered.starts_with('@') {
                write!(f, "\\{rendered}")
            } else {
                write!(f, "{rendered}")
            }
        }
    }
}

#[cfg(feature = "serde")]
impl<TRoot: OmniPathRoot> serde::Serialize for OmniPath<TRoot> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_string().serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de, TRoot: OmniPathRoot> serde::Deserialize<'de> for OmniPath<TRoot> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_str(&s).map_err(|_| {
            serde::de::Error::custom(format!("invalid root in path: {s}"))
        })
    }
}

#[cfg(feature = "merge")]
impl merge::Merge for OmniPath {
    fn merge(&mut self, other: Self) {
        *self = other;
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn confined_paths_are_rooted_relative_plain_names() {
        for ok in ["dist/a.js", "./dist/a.js", "a", "a/./b"] {
            assert!(
                OmniPath::<Root>::new_rooted(ok, Root::Project).is_confined(),
                "{ok:?}"
            );
            assert!(
                OmniPath::<Root>::new_rooted(ok, Root::Workspace).is_confined(),
                "{ok:?}"
            );
        }
    }

    #[test]
    fn windows_unsafe_names_are_recognised_on_every_platform() {
        for bad in [
            "..",
            "...",
            ".. ",
            "a.",
            "a ",
            "a:b",
            "a:stream",
            "C:",
            "a*b",
            "a?b",
            "a<b",
            "a>b",
            "a\"b",
            "a|b",
            "CON",
            "con",
            "NUL",
            "nul.txt",
            "Aux.tar.gz",
            "PRN ",
            "COM1",
            "lpt9",
            "COM3.log",
            "CONIN$",
            "CONOUT$",
        ] {
            assert!(is_windows_unsafe_name(bad), "{bad:?} must be unsafe");
        }
        for good in [
            "a",
            "a.b",
            "dist",
            "output.js.cache",
            "console",
            "CONSOLE",
            "COM10",
            "COM",
            "LPT0x",
            "nulls",
            "a b",
            ".hidden",
            "x..y",
        ] {
            assert!(!is_windows_unsafe_name(good), "{good:?} must be fine");
        }
    }

    #[test]
    fn control_characters_are_never_confined() {
        for bad in ["a\0b", "a\nb", "dir/a\tb", "\u{7f}", "x/\r"] {
            assert!(!is_confined_relative(Path::new(bad)), "{bad:?}");
            assert!(
                !OmniPath::<Root>::new_rooted(bad, Root::Project).is_confined(),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn sibling_paths_stay_within_the_workspace_but_nothing_else() {
        let nested: RootMap = if cfg!(windows) {
            enum_map::enum_map! {
                Root::Workspace => Path::new("E:\\ws"),
                Root::Project => Path::new("E:\\ws\\app"),
            }
        } else {
            enum_map::enum_map! {
                Root::Workspace => Path::new("/ws"),
                Root::Project => Path::new("/ws/app"),
            }
        };
        let n = |p: &str, r: Root| {
            OmniPath::<Root>::new_rooted(p, r).stays_within_workspace(&nested)
        };
        for ok in [
            ("../shared/x", Root::Project),
            ("../shared/*.txt", Root::Project),
            ("dist/../out/x", Root::Project),
            ("shared/x", Root::Workspace),
        ] {
            assert!(n(ok.0, ok.1), "{ok:?}");
        }
        for bad in [
            ("../../external/x", Root::Project),
            ("../..", Root::Project),
            ("..", Root::Project),
            ("../", Root::Project),
            ("", Root::Project),
            ("../../x", Root::Workspace),
            ("a/../../../x", Root::Project),
            ("a\0b", Root::Project),
        ] {
            assert!(!n(bad.0, bad.1), "{bad:?} must not be within");
        }
        assert!(!OmniPath::<Root>::new("a/b").stays_within_workspace(&nested));
        if !cfg!(windows) {
            assert!(!n("/etc/passwd", Root::Project));
        }
    }

    #[test]
    fn escaping_or_anchorless_paths_are_not_confined() {
        let mut bad = vec![
            "",
            ".",
            "./",
            "..",
            "../x",
            "a/../../x",
            "a/..",
            "../../etc/passwd",
        ];
        if cfg!(windows) {
            bad.extend(["C:\\x", "C:x", "\\\\host\\share\\x", "\\x"]);
        } else {
            bad.extend(["/etc/passwd", "//x", "/"]);
        }
        for p in bad {
            assert!(
                !OmniPath::<Root>::new_rooted(p, Root::Project).is_confined(),
                "{p:?} must not be confined"
            );
            assert!(!is_confined_relative(Path::new(p)), "{p:?}");
        }
        // Without a root there is nothing to confine the path to.
        assert!(!OmniPath::<Root>::new("dist/a.js").is_confined());
        assert!(!OmniPath::<Root>::new("/abs").is_confined());
    }

    fn base() -> RootMap<'static> {
        if cfg!(windows) {
            enum_map::enum_map! {
                Root::Workspace => Path::new("E:\\workspace"),
                Root::Project => Path::new("E:\\project"),
            }
        } else {
            enum_map::enum_map! {
                Root::Workspace => Path::new("/workspace"),
                Root::Project => Path::new("/project"),
            }
        }
    }

    #[test]
    fn test_serialize() {
        let path = OmniPath::new_rooted("foo", Root::Workspace);
        assert_eq!(
            serde_json::to_string(&path).unwrap(),
            r#""@workspace/foo""#
        );

        let path = OmniPath::new_rooted("foo", Root::Project);
        assert_eq!(serde_json::to_string(&path).unwrap(), r#""@project/foo""#);

        let path = OmniPath::<Root>::new("foo");
        assert_eq!(serde_json::to_string(&path).unwrap(), r#""foo""#);
    }

    #[test]
    fn test_deserialize() {
        let path = OmniPath::new_rooted("foo", Root::Workspace);
        assert_eq!(
            serde_json::from_str::<OmniPath>(r#""@workspace/foo""#).unwrap(),
            path
        );

        let path = OmniPath::new_rooted("foo", Root::Project);
        assert_eq!(
            serde_json::from_str::<OmniPath>(r#""@project/foo""#).unwrap(),
            path
        );

        let path = OmniPath::new("foo");
        assert_eq!(serde_json::from_str::<OmniPath>(r#""foo""#).unwrap(), path);
    }

    #[test]
    fn test_resolve() {
        let path = OmniPath::new_rooted("foo", Root::Workspace);

        let base = base();

        if cfg!(windows) {
            assert_eq!(path.resolve(&base), Path::new("E:\\workspace\\foo"));
        } else {
            assert_eq!(path.resolve(&base), Path::new("/workspace/foo"));
        }
    }

    #[test]
    fn test_resolve_in_place() {
        let mut path = OmniPath::new_rooted("foo", Root::Workspace);

        let base = base();
        path.resolve_in_place(&base);

        if cfg!(windows) {
            assert_eq!(
                path.path().expect("path should be resolved"),
                Path::new("E:\\workspace\\foo")
            );
        } else {
            assert_eq!(
                path.path().expect("path should be resolved"),
                Path::new("/workspace/foo")
            );
        }
    }

    #[test]
    fn test_to_string() {
        let path = OmniPath::new_rooted("foo", Root::Workspace);
        assert_eq!(path.to_string(), "@workspace/foo");

        let path = OmniPath::new_rooted("foo", Root::Project);
        assert_eq!(path.to_string(), "@project/foo");

        let path = OmniPath::<Root>::new("foo");
        assert_eq!(path.to_string(), "foo");
    }

    #[test]
    fn test_from_str_escapes_literal_at() {
        let path = OmniPath::<Root>::from_str("\\@myorg/pkg").unwrap();
        assert!(!path.is_any_rooted());
        assert_eq!(path.unresolved_path(), Path::new("@myorg/pkg"));
        assert_eq!(path.to_string(), "\\@myorg/pkg");
        assert_eq!(
            OmniPath::<Root>::from_str(&path.to_string()).unwrap(),
            path
        );
    }

    #[test]
    fn test_from_str_bare_root_without_slash() {
        let path = OmniPath::<Root>::from_str("@workspace").unwrap();
        assert!(path.is_rooted(Root::Workspace));
        assert_eq!(path.unresolved_path(), Path::new(""));
    }

    #[test]
    fn test_from_str_rooted_and_unrooted_round_trip() {
        let rooted = OmniPath::<Root>::from_str("@workspace/x").unwrap();
        assert!(rooted.is_rooted(Root::Workspace));
        assert_eq!(rooted.unresolved_path(), Path::new("x"));
        assert_eq!(
            OmniPath::<Root>::from_str(&rooted.to_string()).unwrap(),
            rooted
        );

        let unrooted = OmniPath::<Root>::from_str("a/b").unwrap();
        assert!(!unrooted.is_any_rooted());
        assert_eq!(unrooted.unresolved_path(), Path::new("a/b"));
        assert_eq!(
            OmniPath::<Root>::from_str(&unrooted.to_string()).unwrap(),
            unrooted
        );
    }

    #[test]
    fn test_from_str_rejects_unknown_root() {
        assert!(OmniPath::<Root>::from_str("@nope/x").is_err());
    }

    #[test]
    fn test_serde_escapes_literal_at() {
        let json = r#""\\@myorg/pkg""#;
        let path: OmniPath<Root> = serde_json::from_str(json).unwrap();
        assert!(!path.is_any_rooted());
        assert_eq!(path.unresolved_path(), Path::new("@myorg/pkg"));
        assert_eq!(serde_json::to_string(&path).unwrap(), json);
    }
}
