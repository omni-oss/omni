use std::{
    fmt,
    path::{Path, PathBuf},
};

use omni_path_utils::ProjectRootId;

const LOGICAL_PREFIX: &str = "project:";

/// Stable synthetic identity of a project, derived from its real root.
///
/// It is not a path: it deliberately has no `AsRef<Path>` or
/// `Into<PathBuf>`, so it cannot reach file-read or hash-input APIs.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct LogicalProjectKey(ProjectRootId);

impl LogicalProjectKey {
    pub fn new(root: ProjectRootId) -> Self {
        Self(root)
    }

    pub fn root(&self) -> &ProjectRootId {
        &self.0
    }
}

impl fmt::Display for LogicalProjectKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{LOGICAL_PREFIX}{}", self.0)
    }
}

/// Where a project's configuration lives: a real file, or nowhere
/// (virtual, identified only by its logical key).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ConfigLocation {
    File(PathBuf),
    Virtual(LogicalProjectKey),
}

impl ConfigLocation {
    /// The only way to obtain a path for file reads or hash inputs.
    pub fn file_path(&self) -> Option<&Path> {
        match self {
            Self::File(p) => Some(p),
            Self::Virtual(_) => None,
        }
    }

    pub fn is_virtual(&self) -> bool {
        matches!(self, Self::Virtual(_))
    }

    pub fn logical_key(&self) -> Option<&LogicalProjectKey> {
        match self {
            Self::Virtual(k) => Some(k),
            Self::File(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use omni_path_utils::WorkspacePath;

    use super::*;

    fn key(p: &str) -> LogicalProjectKey {
        LogicalProjectKey::new(ProjectRootId::new(
            WorkspacePath::parse(p).unwrap(),
        ))
    }

    #[test]
    fn key_is_stable_and_distinct_per_root() {
        assert_eq!(key("apps/a"), key("apps/a"));
        assert_ne!(key("apps/a"), key("apps/b"));
        assert_eq!(key("apps/a").to_string(), "project:apps/a");
    }

    #[test]
    fn virtual_location_never_yields_a_file_path() {
        let loc = ConfigLocation::Virtual(key("apps/a"));
        assert!(loc.is_virtual());
        assert_eq!(loc.file_path(), None);
        assert_eq!(loc.logical_key(), Some(&key("apps/a")));
    }

    #[test]
    fn file_location_round_trips_path_without_key() {
        let loc =
            ConfigLocation::File(PathBuf::from("/w/apps/a/project.omni.yaml"));
        assert!(!loc.is_virtual());
        assert_eq!(
            loc.file_path(),
            Some(Path::new("/w/apps/a/project.omni.yaml"))
        );
        assert_eq!(loc.logical_key(), None);
    }

    #[test]
    fn key_text_is_not_a_real_path_form() {
        // The rendered key must never be mistaken for a workspace file.
        let s = key("apps/a").to_string();
        assert!(!Path::new(&s).is_absolute());
        assert!(s.starts_with(LOGICAL_PREFIX));
    }
}
