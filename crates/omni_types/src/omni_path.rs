use std::{
    borrow::Cow,
    fmt::{Debug as _, Display},
    path::{Path as StdPath, PathBuf},
};

use enum_map::EnumMap;
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
)]
pub enum Root {
    Workspace,
    Project,
}

pub type RootMap<'a> = EnumMap<Root, &'a StdPath>;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct OmniPath {
    path: PathBuf,
    root: Option<Root>,
}

#[cfg(feature = "schemars")]
impl schemars::JsonSchema for OmniPath {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "OmniPath".into()
    }

    fn json_schema(
        generator: &mut schemars::SchemaGenerator,
    ) -> schemars::Schema {
        String::json_schema(generator)
    }
}

impl Display for OmniPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(root) = self.root {
            write!(f, "@{}/{}", root, self.path.display())
        } else {
            self.path.fmt(f)
        }
    }
}

impl OmniPath {
    pub fn new_rooted(path: impl Into<PathBuf>, root: Root) -> Self {
        Self {
            path: path.into(),
            root: Some(root),
        }
    }

    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            root: None,
        }
    }

    pub fn new_ws_rooted(path: impl Into<PathBuf>) -> Self {
        Self::new_rooted(path, Root::Workspace)
    }

    pub fn new_project_rooted(path: impl Into<PathBuf>) -> Self {
        Self::new_rooted(path, Root::Project)
    }

    pub fn root(&self) -> Option<Root> {
        self.root
    }

    pub fn is_rooted(&self) -> bool {
        self.root.is_some()
    }

    pub fn is_ws_rooted(&self) -> bool {
        self.root.map(|r| r == Root::Workspace).unwrap_or(false)
    }

    pub fn is_project_rooted(&self) -> bool {
        self.root.map(|r| r == Root::Project).unwrap_or(false)
    }

    pub fn path(&self) -> Result<&StdPath, OmniPathError> {
        if self.root.is_some() {
            Err(OmniPathErrorInner::NotResolved.into())
        } else {
            Ok(&self.path)
        }
    }

    pub fn unresolved_path(&self) -> &StdPath {
        &self.path
    }

    /// Resolves the path relative to the given base
    pub fn resolve<'a>(
        &'a self,
        base: &EnumMap<Root, &StdPath>,
    ) -> Cow<'a, StdPath> {
        if let Some(root) = self.root {
            Cow::Owned(base[root].join(&self.path))
        } else {
            Cow::Borrowed(&self.path)
        }
    }

    pub fn resolve_in_place(&mut self, base: &EnumMap<Root, &StdPath>) {
        if let Some(root) = self.root {
            *self = Self {
                path: base[root].join(&self.path),
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

#[cfg(feature = "serde")]
impl serde::Serialize for OmniPath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if self.is_ws_rooted() {
            format!("@workspace/{}", self.path.to_string_lossy())
                .serialize(serializer)
        } else if self.is_project_rooted() {
            format!("@project/{}", self.path.to_string_lossy())
                .serialize(serializer)
        } else {
            self.path.serialize(serializer)
        }
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for OmniPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if s.starts_with("@workspace/") {
            Ok(Self::new_ws_rooted(PathBuf::from(
                s.strip_prefix("@workspace/").unwrap(),
            )))
        } else if s.starts_with("@project/") {
            Ok(Self::new_project_rooted(PathBuf::from(
                s.strip_prefix("@project/").unwrap(),
            )))
        } else {
            Ok(Self::new(s))
        }
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
    fn test_serialize() {
        let path = OmniPath::new_ws_rooted("foo");
        assert_eq!(
            serde_json::to_string(&path).unwrap(),
            r#""@workspace/foo""#
        );

        let path = OmniPath::new_project_rooted("foo");
        assert_eq!(serde_json::to_string(&path).unwrap(), r#""@project/foo""#);

        let path = OmniPath::new("foo");
        assert_eq!(serde_json::to_string(&path).unwrap(), r#""foo""#);
    }

    #[test]
    fn test_deserialize() {
        let path = OmniPath::new_ws_rooted("foo");
        assert_eq!(
            serde_json::from_str::<OmniPath>(r#""@workspace/foo""#).unwrap(),
            path
        );

        let path = OmniPath::new_project_rooted("foo");
        assert_eq!(
            serde_json::from_str::<OmniPath>(r#""@project/foo""#).unwrap(),
            path
        );

        let path = OmniPath::new("foo");
        assert_eq!(serde_json::from_str::<OmniPath>(r#""foo""#).unwrap(), path);
    }

    #[test]
    fn test_resolve() {
        let path = OmniPath::new_ws_rooted("foo");

        let base = enum_map::enum_map! {
            Root::Workspace => Path::new("/workspace"),
            Root::Project => Path::new("/project"),
        };

        assert_eq!(path.resolve(base), Path::new("/workspace/foo"));
    }

    #[test]
    fn test_resolve_in_place() {
        let mut path = OmniPath::new_ws_rooted("foo");

        let base = enum_map::enum_map! {
            Root::Workspace => Path::new("/workspace"),
            Root::Project => Path::new("/project"),
        };

        path.resolve_in_place(base);

        assert_eq!(
            path.path().expect("path should be resolved"),
            Path::new("/workspace/foo")
        );
    }
}
