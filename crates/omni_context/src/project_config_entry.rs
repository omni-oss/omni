use std::path::Path;

use omni_configurations::{FinalizedProject, ProjectConfiguration};
use omni_types::OmniPath;
use sets::UnorderedSet;

use crate::{ConfigLocation, LogicalProjectKey};

/// A loaded project configuration tagged with whether project discovery
/// found it, as opposed to it being reached only through `extends`.
///
/// The authored `config` holds no location: the project's root directory and
/// config location are separate, resolved values.
#[derive(Debug, Clone)]
pub struct ProjectConfigEntry {
    pub config: ProjectConfiguration,
    /// The directory the project runs in. Not the config file's directory
    /// when that is unknown (virtual) and not a declaration's path base.
    pub root_dir: OmniPath,
    /// Where the configuration lives: a real file, or nowhere (virtual).
    pub location: ConfigLocation,
    pub discovered: bool,
}

impl ProjectConfigEntry {
    /// An entry read from the concrete file of a finalized project.
    ///
    /// Returns `None` when the project has no file, which discovery never
    /// produces.
    pub(crate) fn from_finalized(
        project: FinalizedProject,
        discovered: bool,
    ) -> Option<Self> {
        let file = project.file.path().ok()?.to_path_buf();
        Some(Self {
            config: project.config,
            root_dir: project.dir,
            location: ConfigLocation::File(file),
            discovered,
        })
    }

    /// A discovered project that has no configuration file.
    ///
    /// Nothing in discovery produces these yet; it exists so the virtual
    /// representation can be exercised without a plugin.
    pub fn new_virtual(
        config: ProjectConfiguration,
        root_dir: OmniPath,
        key: LogicalProjectKey,
    ) -> Self {
        Self {
            config,
            root_dir,
            location: ConfigLocation::Virtual(key),
            discovered: true,
        }
    }

    /// The config file path, or `None` for a virtual project. This is the
    /// only way to reach a file read from an entry.
    pub fn file_path(&self) -> Option<&Path> {
        self.location.file_path()
    }

    /// Whether the task pipeline treats this config as a project.
    pub fn is_runnable(&self) -> bool {
        is_runnable(&self.config, self.discovered)
    }
}

pub(crate) fn is_discovered(
    project: &FinalizedProject,
    discovered: &UnorderedSet<&Path>,
) -> bool {
    discovered.contains(project.file.path().expect("path should be resolved"))
}

pub(crate) fn is_runnable_project(
    project: &FinalizedProject,
    discovered: &UnorderedSet<&Path>,
) -> bool {
    is_runnable(&project.config, is_discovered(project, discovered))
}

fn is_runnable(config: &ProjectConfiguration, discovered: bool) -> bool {
    discovered && !config.base
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use omni_configurations::LoadedLayer;

    use super::*;

    fn entry(base: bool, discovered: bool) -> ProjectConfigEntry {
        let mut entry = ProjectConfigEntry::new_virtual(
            serde_json::from_value(serde_json::json!({
                "name": "project",
                "base": base,
            }))
            .unwrap(),
            OmniPath::from(std::path::PathBuf::from("/w/apps/project")),
            LogicalProjectKey::new(omni_path_utils::ProjectRootId::new(
                omni_path_utils::WorkspacePath::parse("apps/project").unwrap(),
            )),
        );
        entry.discovered = discovered;
        entry
    }

    #[test]
    fn virtual_entry_has_no_file_path_and_is_runnable() {
        let e = entry(false, true);

        assert!(e.location.is_virtual());
        assert_eq!(e.file_path(), None);
        assert!(e.is_runnable());
    }

    #[test]
    fn virtual_entry_has_a_root_directory_but_no_file() {
        let e = entry(false, true);

        assert_eq!(e.root_dir.path().unwrap(), Path::new("/w/apps/project"));
        assert_eq!(e.file_path(), None);
    }

    #[test]
    fn file_entry_exposes_its_config_file_and_root() {
        let finalized = LoadedLayer::from_file(
            PathBuf::from("/w/a/project.omni.yaml").into(),
            PathBuf::from("/w/a").into(),
            serde_json::from_value(serde_json::json!({ "name": "project" }))
                .unwrap(),
        )
        .finalize_composed()
        .unwrap();

        let e = ProjectConfigEntry::from_finalized(finalized, true).unwrap();

        assert_eq!(e.file_path(), Some(Path::new("/w/a/project.omni.yaml")));
        assert_eq!(e.root_dir.path().unwrap(), Path::new("/w/a"));
        assert!(!e.location.is_virtual());
    }

    #[test]
    fn test_is_runnable_requires_discovered_and_not_base() {
        let cases = [
            (false, true, true),
            (true, true, false),
            (false, false, false),
            (true, false, false),
        ];

        for (base, discovered, expected) in cases {
            assert_eq!(
                entry(base, discovered).is_runnable(),
                expected,
                "base: {base}, discovered: {discovered}"
            );
        }
    }
}
