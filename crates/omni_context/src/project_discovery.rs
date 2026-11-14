use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::LazyLock,
};

use derive_new::new;
use dir_walker::DirWalker;
use omni_configuration_discovery::ConfigurationDiscovery;
use omni_path_utils::{ProjectRootId, WorkspacePath};
use thiserror::Error;

use crate::{ConfigLocation, LogicalProjectKey, constants};
use crate::{ContextError, ContextSys, context::ContextErrorInner};

/// Validate only selected concrete files, never the `extends` closure.
#[allow(clippy::result_large_err)]
pub(crate) fn validate_project_roots<TSys: ContextSys>(
    files: &[PathBuf],
    sys: &TSys,
) -> Result<(), ContextError> {
    let mut roots: BTreeMap<PathBuf, BTreeSet<PathBuf>> = BTreeMap::new();
    for file in files {
        if let Some(parent) = file.parent() {
            let root = sys.fs_canonicalize(parent)?;
            roots.entry(root).or_default().insert(file.clone());
        }
    }
    let conflicts: BTreeMap<_, _> = roots
        .into_iter()
        .filter(|(_, files)| files.len() > 1)
        .collect();
    if conflicts.is_empty() {
        Ok(())
    } else {
        Err(ContextErrorInner::DuplicateProjectFiles { conflicts }.into())
    }
}

#[derive(Debug, Clone)]
pub struct ProjectDiscovery<'a> {
    discovery: ConfigurationDiscovery<'a, String, String, String>,
}

static IGNORE_FILES: LazyLock<[String; 1]> =
    LazyLock::new(|| [constants::OMNI_IGNORE.to_string()]);

static CONFIG_FILES: LazyLock<Vec<String>> = LazyLock::new(|| {
    constants::SUPPORTED_EXTENSIONS
        .iter()
        .map(|ext| constants::PROJECT_OMNI.replace("{ext}", ext))
        .collect()
});

impl<'a> ProjectDiscovery<'a> {
    pub fn new(
        root_dir: &'a Path,
        include_patterns: &'a [String],
        exclude_patterns: &'a [String],
    ) -> Self {
        Self {
            discovery: ConfigurationDiscovery::new(
                root_dir,
                include_patterns,
                exclude_patterns,
                &CONFIG_FILES[..],
                &IGNORE_FILES[..],
                "project",
            ),
        }
    }
}

impl<'a> ProjectDiscovery<'a> {
    pub async fn discover_project_files_with_walker<TDirWalker: DirWalker>(
        &self,
        walker: &TDirWalker,
    ) -> Result<Vec<DiscoveredPath>, ProjectDiscoveryError> {
        let discovered = self.discovery.discover_with_walker(walker).await?;

        Ok(discovered
            .into_iter()
            .map(DiscoveredPath::new_real)
            .collect())
    }
}

#[derive(Debug, Clone, new)]
pub enum DiscoveredPath {
    Real {
        #[new(into)]
        file: PathBuf,
    },
    /// A project with no config file. No discovery source produces these yet.
    #[allow(unused)]
    Virtual {
        #[new(into)]
        dir: PathBuf,
    },
}

/// Discovery results split by how each project's configuration is located.
#[derive(Debug, Default)]
pub(crate) struct ClassifiedPaths {
    /// Real config files, the only paths that may be read or hashed.
    pub files: Vec<PathBuf>,
    /// Projects with no config file, identified by their logical key.
    pub virtual_locations: Vec<ConfigLocation>,
}

impl ClassifiedPaths {
    /// Split `paths`, keeping only real files accepted by `keep_file`.
    /// A virtual project outside `root_dir` has no valid key and is dropped.
    pub fn classify(
        paths: Vec<DiscoveredPath>,
        root_dir: &Path,
        keep_file: impl Fn(&Path) -> bool,
    ) -> Self {
        let mut out = Self::default();
        for path in paths {
            match path {
                DiscoveredPath::Real { file } => {
                    if keep_file(&file) {
                        out.files.push(file);
                    }
                }
                DiscoveredPath::Virtual { dir } => {
                    match WorkspacePath::from_os_path(root_dir, &dir) {
                        Ok(root) => out.virtual_locations.push(
                            ConfigLocation::Virtual(LogicalProjectKey::new(
                                ProjectRootId::new(root),
                            )),
                        ),
                        Err(error) => tracing::warn!(
                            ?dir,
                            %error,
                            "ignoring virtual project outside the workspace"
                        ),
                    }
                }
            }
        }
        out
    }
}

#[derive(Error, Debug, omni_diagnostic::Diagnostic)]
#[error(transparent)]
#[diagnostic(delegate)]
pub struct ProjectDiscoveryError(
    #[from] pub(crate) omni_configuration_discovery::error::Error,
);

#[cfg(test)]
mod tests {
    use omni_diagnostic::Diagnostic as _;
    use system_traits::impls::RealSys;

    use super::*;

    #[test]
    fn duplicate_roots_report_all_formats_in_stable_order() {
        let dir = tempfile::tempdir().unwrap();
        let mut files = vec![];
        for project in ["a", "b"] {
            let root = dir.path().join(project);
            std::fs::create_dir(&root).unwrap();
            for ext in ["yaml", "yml", "json", "toml"] {
                files.push(root.join(format!("project.omni.{ext}")));
            }
        }
        for count in [2, 3, 4, 8] {
            let mut selected = files[..count].to_vec();
            let error =
                validate_project_roots(&selected, &RealSys).unwrap_err();
            assert_eq!(error.code(), "workspace.duplicate_project_files");
            let details = error.details().unwrap();
            let conflicts = details["conflicts"].as_object().unwrap();
            assert_eq!(
                conflicts
                    .values()
                    .map(|v| v.as_array().unwrap().len())
                    .sum::<usize>(),
                count
            );
            for file in &selected {
                assert!(error.to_string().contains(&format!("{file:?}")));
            }
            assert!(error.hints()[0].contains("projects"));
            selected.reverse();
            let reversed =
                validate_project_roots(&selected, &RealSys).unwrap_err();
            assert_eq!(error.to_string(), reversed.to_string());
            assert_eq!(error.details(), reversed.details());
        }
    }

    #[test]
    fn separate_roots_and_repeated_hits_are_not_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let child = dir.path().join("child");
        std::fs::create_dir(&child).unwrap();
        let file = dir.path().join("project.omni.yaml");
        validate_project_roots(
            &[file.clone(), file, child.join("project.omni.json")],
            &RealSys,
        )
        .unwrap();
        validate_project_roots(&[], &RealSys).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn parent_symlinks_share_a_canonical_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("real");
        let alias = dir.path().join("alias");
        std::fs::create_dir(&root).unwrap();
        std::os::unix::fs::symlink(&root, &alias).unwrap();
        let error = validate_project_roots(
            &[
                root.join("project.omni.yaml"),
                alias.join("project.omni.json"),
            ],
            &RealSys,
        )
        .unwrap_err();
        let details = error.details().unwrap();
        let canonical = std::fs::canonicalize(root).unwrap();
        assert!(
            details["conflicts"]
                .get(canonical.to_str().unwrap())
                .is_some()
        );
    }

    #[test]
    fn classification_keeps_files_and_virtual_projects_apart() {
        let root = Path::new("/w");
        let paths = vec![
            DiscoveredPath::new_real("/w/a/project.omni.yaml"),
            DiscoveredPath::new_virtual("/w/b"),
            DiscoveredPath::new_real("/w/.cache/x/project.omni.yaml"),
        ];

        let out = ClassifiedPaths::classify(paths, root, |p| {
            !p.starts_with("/w/.cache")
        });

        assert_eq!(out.files, vec![PathBuf::from("/w/a/project.omni.yaml")]);
        assert_eq!(out.virtual_locations.len(), 1);
        let loc = &out.virtual_locations[0];
        assert_eq!(loc.file_path(), None);
        assert_eq!(loc.logical_key().unwrap().to_string(), "project:b");
    }

    #[test]
    fn virtual_project_outside_the_workspace_is_dropped() {
        let out = ClassifiedPaths::classify(
            vec![DiscoveredPath::new_virtual("/elsewhere/b")],
            Path::new("/w"),
            |_| true,
        );

        assert!(out.files.is_empty());
        assert!(out.virtual_locations.is_empty());
    }
}
