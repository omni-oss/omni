use std::path::Path;

use omni_configurations::ProjectConfiguration;
use sets::UnorderedSet;

/// A loaded project configuration tagged with whether project discovery
/// found it, as opposed to it being reached only through `extends`.
#[derive(Debug, Clone)]
pub struct ProjectConfigEntry {
    pub config: ProjectConfiguration,
    pub discovered: bool,
}

impl ProjectConfigEntry {
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
        ProjectConfigEntry {
            config: serde_json::from_value(serde_json::json!({
                "name": "project",
                "base": base,
            }))
            .unwrap(),
            discovered,
        }
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
