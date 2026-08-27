//! Neutral, dependency-free home for constants shared across omni crates: the
//! configuration file extensions omni recognizes, the `.omniignore` name, and
//! the `<stem>.omni.{ext}` manifest name templates.
//!
//! Crate-specific constants (cache/scratch directory names, environment-variable
//! names, regexes) stay in their owning crates; only the values that must agree
//! across subsystems live here.

/// The file extensions omni recognizes for hand-authored configuration, in
/// discovery priority order. The binary cache format is deliberately excluded:
/// it is a machine-only representation, never a configuration manifest.
pub const SUPPORTED_CONFIG_EXTS: &[&str] = &["yml", "yaml", "json", "toml"];

/// The per-directory ignore file honored by configuration discovery.
pub const OMNI_IGNORE: &str = ".omniignore";

/// `<stem>.omni.{ext}` manifest name templates. `{ext}` is expanded with
/// [`config_file_names`] (or a caller-chosen extension via `str::replace`).
pub const WORKSPACE_OMNI: &str = "workspace.omni.{ext}";
pub const PROJECT_OMNI: &str = "project.omni.{ext}";
pub const GENERATOR_OMNI: &str = "generator.omni.{ext}";
pub const TOOL_OMNI: &str = "tool.omni.{ext}";
pub const PROJECTION_OMNI: &str = "projection.omni.{ext}";
pub const REMOTE_CACHE_OMNI: &str = "remote-cache.omni.{ext}";

/// The manifest templates that make up omni's control plane. Projecting a
/// source entry onto any of these is refused unless explicitly allowed.
pub const CONTROL_PLANE_MANIFESTS: &[&str] =
    &[WORKSPACE_OMNI, PROJECT_OMNI, TOOL_OMNI, GENERATOR_OMNI];

/// Expand a `{ext}` manifest template across every supported configuration
/// extension, yielding the concrete file names in discovery priority order.
pub fn config_file_names(template: &str) -> Vec<String> {
    SUPPORTED_CONFIG_EXTS
        .iter()
        .map(|ext| template.replace("{ext}", ext))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_file_names_expand_every_extension_in_order() {
        assert_eq!(
            config_file_names(GENERATOR_OMNI),
            vec![
                "generator.omni.yml",
                "generator.omni.yaml",
                "generator.omni.json",
                "generator.omni.toml",
            ]
        );
    }

    #[test]
    fn every_template_carries_the_ext_placeholder() {
        for template in [
            WORKSPACE_OMNI,
            PROJECT_OMNI,
            GENERATOR_OMNI,
            TOOL_OMNI,
            PROJECTION_OMNI,
            REMOTE_CACHE_OMNI,
        ] {
            assert!(
                template.contains("{ext}"),
                "template `{template}` must carry an `{{ext}}` placeholder"
            );
        }
    }

    #[test]
    fn control_plane_manifests_are_the_config_loading_stems() {
        assert_eq!(
            CONTROL_PLANE_MANIFESTS,
            &[WORKSPACE_OMNI, PROJECT_OMNI, TOOL_OMNI, GENERATOR_OMNI]
        );
    }
}
