use std::path::Path;

use crate::error::{ProjectionError, Result};

const MANIFEST_STEMS: [&str; 4] = ["workspace", "project", "tool", "generator"];
const MANIFEST_EXTS: [&str; 4] = ["yml", "yaml", "json", "toml"];

/// Plan-time safety policy for a single projection.
pub struct Guardrails<'a> {
    pub allow_omni_config: bool,
    pub allow_git: bool,
    /// Resolved workspace env-file names (from `WorkspaceEnvConfiguration.files`),
    /// supplied by the caller rather than hardcoded.
    pub env_files: &'a [String],
}

/// Reject a destination that would collide with omni's control plane (config
/// another subsystem loads) or land inside a `.git` directory, unless the
/// matching `allow_*` escape hatch is set.
pub fn check_dest(dest_abs: &Path, guardrails: &Guardrails) -> Result<()> {
    if !guardrails.allow_git && has_git_component(dest_abs) {
        return Err(ProjectionError::custom(format!(
            "destination is inside a `.git` directory (set `allow_git: true` to override): {}",
            dest_abs.display()
        )));
    }

    if !guardrails.allow_omni_config {
        let basename = dest_abs
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();

        if is_control_plane_basename(&basename)
            || is_env_file(&basename, guardrails.env_files)
        {
            return Err(ProjectionError::custom(format!(
                "destination would overwrite omni control-plane config (set `allow_omni_config: true` to override): {}",
                dest_abs.display()
            )));
        }
    }

    Ok(())
}

fn has_git_component(path: &Path) -> bool {
    path.components().any(|c| c.as_os_str() == ".git")
}

fn is_control_plane_basename(basename: &str) -> bool {
    if basename == ".omniignore" {
        return true;
    }
    if basename.starts_with("remote-cache.omni.") {
        return true;
    }

    for stem in MANIFEST_STEMS {
        for ext in MANIFEST_EXTS {
            if basename == format!("{stem}.omni.{ext}") {
                return true;
            }
        }
    }

    false
}

fn is_env_file(basename: &str, env_files: &[String]) -> bool {
    env_files.iter().any(|f| {
        Path::new(f)
            .file_name()
            .map(|name| name == basename)
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guardrails<'a>(env_files: &'a [String]) -> Guardrails<'a> {
        Guardrails {
            allow_omni_config: false,
            allow_git: false,
            env_files,
        }
    }

    #[test]
    fn refuses_control_plane_manifests() {
        let g = guardrails(&[]);
        for name in [
            "workspace.omni.yaml",
            "project.omni.json",
            "tool.omni.toml",
            "generator.omni.yml",
            ".omniignore",
            "remote-cache.omni.bin",
        ] {
            let dest = Path::new("/ws").join(name);
            assert!(check_dest(&dest, &g).is_err(), "`{name}` must be refused");
        }
    }

    #[test]
    fn allows_ordinary_destinations() {
        let g = guardrails(&[]);
        assert!(check_dest(Path::new("/ws/.cursor/rules/foo.md"), &g).is_ok());
    }

    #[test]
    fn refuses_resolved_env_files() {
        let env = vec![".env".to_string(), "config/.env.local".to_string()];
        let g = guardrails(&env);
        assert!(check_dest(Path::new("/ws/.env"), &g).is_err());
        assert!(check_dest(Path::new("/ws/sub/.env.local"), &g).is_err());
        assert!(check_dest(Path::new("/ws/other.txt"), &g).is_ok());
    }

    #[test]
    fn refuses_git_directory_unless_allowed() {
        let env: Vec<String> = vec![];
        let g = guardrails(&env);
        let dest = Path::new("/ws/.git/hooks/pre-commit");
        assert!(check_dest(dest, &g).is_err());

        let allowed = Guardrails {
            allow_omni_config: false,
            allow_git: true,
            env_files: &env,
        };
        assert!(check_dest(dest, &allowed).is_ok());
    }

    #[test]
    fn allow_omni_config_overrides_manifest_refusal() {
        let env: Vec<String> = vec![];
        let allowed = Guardrails {
            allow_omni_config: true,
            allow_git: false,
            env_files: &env,
        };
        assert!(
            check_dest(Path::new("/ws/workspace.omni.yaml"), &allowed).is_ok()
        );
    }
}
