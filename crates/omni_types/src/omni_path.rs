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
