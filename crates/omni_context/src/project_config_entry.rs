use std::path::Path;

use omni_configurations::ProjectConfiguration;
use sets::UnorderedSet;

use crate::{ConfigLocation, LogicalProjectKey};

/// A loaded project configuration tagged with whether project discovery
/// found it, as opposed to it being reached only through `extends`.
#[derive(Debug, Clone)]
pub struct ProjectConfigEntry {
    pub config: ProjectConfiguration,
    /// Where the configuration lives: a real file, or nowhere (virtual).
    pub location: ConfigLocation,
    pub discovered: bool,
}

impl ProjectConfigEntry {
    /// An entry read from `config.file`.
    pub(crate) fn from_file(
        config: ProjectConfiguration,
        discovered: bool,
    ) -> Self {
        let file = config
            .file
            .path()
            .expect("path should be resolved")
            .to_path_buf();
        Self {
            config,
            location: ConfigLocation::File(file),
            discovered,
        }
    }

    /// A discovered project that has no configuration file.
    ///
    /// Nothing in discovery produces these yet; it exists so the virtual
    /// representation can be exercised without a plugin.
    pub fn new_virtual(
        config: ProjectConfiguration,
        key: LogicalProjectKey,
    ) -> Self {
        Self {
            config,
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
    config: &ProjectConfiguration,
    discovered: &UnorderedSet<&Path>,
) -> bool {
    discovered.contains(config.file.path().expect("path should be resolved"))
}

pub(crate) fn is_runnable_project(
    config: &ProjectConfiguration,
    discovered: &UnorderedSet<&Path>,
) -> bool {
    is_runnable(config, is_discovered(config, discovered))
}

fn is_runnable(config: &ProjectConfiguration, discovered: bool) -> bool {
    discovered && !config.base
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(base: bool, discovered: bool) -> ProjectConfigEntry {
        let mut entry = ProjectConfigEntry::new_virtual(
            serde_json::from_value(serde_json::json!({
                "name": "project",
                "base": base,
            }))
            .unwrap(),
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
    fn file_entry_exposes_its_config_file() {
        let mut config: ProjectConfiguration =
            serde_json::from_value(serde_json::json!({ "name": "project" }))
                .unwrap();
        config.file = std::path::PathBuf::from("/w/a/project.omni.yaml").into();

        let e = ProjectConfigEntry::from_file(config, true);

        assert_eq!(e.file_path(), Some(Path::new("/w/a/project.omni.yaml")));
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
