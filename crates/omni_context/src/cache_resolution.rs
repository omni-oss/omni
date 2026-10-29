use std::path::{Path, PathBuf};

use omni_configurations::WorktreeConfiguration;
use omni_constants::{
    OMNI_CACHE_DIR, WORKSPACE_OMNI, WORKTREE_OMNI, config_file_names,
};
use omni_scm::CheckoutLayout;

use crate::{
    ContextSys,
    context::{ContextError, ContextErrorInner, canonicalize_cache_dir},
};

/// Construction inputs retained across reloads. Discovery is deliberately not
/// part of resolution, so ordinary reload never consults VCS metadata.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CacheResolutionState {
    explicit: Option<PathBuf>,
    primary: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CacheDirectorySource {
    Explicit,
    WorktreeFile(PathBuf),
    PrimaryFile(PathBuf),
    PrimaryDefault,
    LocalDefault,
}

impl CacheResolutionState {
    pub(crate) fn new(
        root: &Path,
        explicit: Option<PathBuf>,
        sys: &impl ContextSys,
        discover: impl FnOnce(&Path) -> eyre::Result<Option<CheckoutLayout>>,
    ) -> Self {
        if let Some(path) = explicit {
            let path = if path.is_absolute() || empty_path(&path) {
                path
            } else {
                sys.env_current_dir().unwrap_or_default().join(path)
            };
            return Self {
                explicit: Some(path),
                primary: None,
            };
        }
        let primary = match discover(root) {
            Ok(Some(layout)) => mapped_workspace(root, layout, sys),
            Ok(None) => {
                log::debug!("No supported checkout; using local cache policy");
                None
            }
            Err(error) => {
                log::debug!(
                    "Checkout discovery failed; using local cache policy: {error:#}"
                );
                None
            }
        };
        Self {
            explicit: None,
            primary,
        }
    }

    pub(crate) fn configuration_paths(&self, root: &Path) -> Vec<PathBuf> {
        if self.explicit.is_some() {
            return vec![];
        }
        std::iter::once(root)
            .chain(self.primary.as_deref().filter(|primary| *primary != root))
            .flat_map(|root| {
                config_file_names(WORKTREE_OMNI)
                    .into_iter()
                    .map(move |name| root.join(name))
            })
            .collect()
    }

    #[allow(clippy::result_large_err)]
    pub(crate) fn resolve(
        &self,
        root: &Path,
        sys: &impl ContextSys,
    ) -> Result<(PathBuf, CacheDirectorySource), ContextError> {
        let (path, source) = if let Some(path) = &self.explicit {
            if empty_path(path) {
                return Err(ContextErrorInner::CacheDirectory(
                    "explicit cache directory must be a nonempty path".into(),
                )
                .into());
            }
            (path.clone(), CacheDirectorySource::Explicit)
        } else if let Some((path, file)) = configured_directory(root, sys)? {
            (path, CacheDirectorySource::WorktreeFile(file))
        } else if let Some(primary) =
            self.primary.as_deref().filter(|primary| *primary != root)
        {
            if let Some((path, file)) = configured_directory(primary, sys)? {
                (path, CacheDirectorySource::PrimaryFile(file))
            } else {
                (
                    primary.join(OMNI_CACHE_DIR),
                    CacheDirectorySource::PrimaryDefault,
                )
            }
        } else {
            (
                root.join(OMNI_CACHE_DIR),
                if self.primary.is_some() {
                    CacheDirectorySource::PrimaryDefault
                } else {
                    CacheDirectorySource::LocalDefault
                },
            )
        };
        let path = canonicalize_cache_dir(&path, sys);
        for workspace in std::iter::once(root).chain(self.primary.as_deref()) {
            if workspace.starts_with(&path) {
                return Err(ContextErrorInner::CacheDirectory(format!(
                    "unsafe cache directory {} ({source:?}): must not equal or contain workspace {}; choose a dedicated cache directory",
                    path.display(), workspace.display()
                )).into());
            }
        }
        log::debug!(
            "Cache directory {} selected from {source:?}",
            path.display()
        );
        Ok((path, source))
    }
}

fn empty_path(path: &Path) -> bool {
    path.to_str().is_some_and(|value| value.trim().is_empty())
}

fn mapped_workspace(
    root: &Path,
    layout: CheckoutLayout,
    sys: &impl ContextSys,
) -> Option<PathBuf> {
    let mapped = (|| {
        let relative = root.strip_prefix(&layout.checkout_root).ok()?;
        let primary = sys
            .fs_canonicalize(layout.primary_checkout_root.as_ref()?)
            .ok()?;
        let mapped = sys.fs_canonicalize(primary.join(relative)).ok()?;
        if !mapped.starts_with(&primary) {
            return None;
        }
        config_file_names(WORKSPACE_OMNI)
            .iter()
            .any(|name| sys.fs_is_file(mapped.join(name)).unwrap_or(false))
            .then_some(mapped)
    })();
    if mapped.is_none() {
        log::debug!(
            "No safe corresponding primary workspace; using local cache policy"
        );
    }
    mapped
}

#[allow(clippy::result_large_err)]
fn configured_directory(
    root: &Path,
    sys: &impl ContextSys,
) -> Result<Option<(PathBuf, PathBuf)>, ContextError> {
    let mut files = Vec::new();
    for name in config_file_names(WORKTREE_OMNI) {
        let path = root.join(name);
        if sys.fs_exists(&path)? {
            files.push(path);
        }
    }
    if files.len() > 1 {
        return Err(ContextErrorInner::CacheDirectory(format!(
            "multiple worktree configuration files in {}: {files:?}",
            root.display()
        ))
        .into());
    }
    let Some(file) = files.pop() else {
        return Ok(None);
    };
    let config = WorktreeConfiguration::load(&file, sys).map_err(|error| {
        ContextErrorInner::WorktreeConfiguration(file.clone(), error)
    })?;
    let Some(dir) = config.cache.and_then(|cache| cache.dir) else {
        return Ok(None);
    };
    let expansion = dir == "~"
        || dir.starts_with("~/")
        || dir.starts_with("~\\")
        || dir.split('$').skip(1).any(|tail| {
            tail.starts_with('{')
                || tail
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        })
        || dir.split('%').skip(1).step_by(2).any(|tail| {
            !tail.is_empty()
                && tail.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        }) && dir.matches('%').count() >= 2;
    if dir.trim().is_empty() || expansion {
        return Err(ContextErrorInner::CacheDirectory(format!("invalid cache.dir in {}: use a nonempty literal path; home/environment expansion is not supported", file.display())).into());
    }
    #[cfg(windows)]
    if !std::path::Path::new(&dir).has_root()
        && matches!(
            std::path::Path::new(&dir).components().next(),
            Some(std::path::Component::Prefix(_))
        )
    {
        return Err(ContextErrorInner::CacheDirectory(format!(
            "invalid cache.dir in {}: drive-relative paths are ambiguous; use a fully absolute or workspace-relative path", file.display()
        )).into());
    }
    Ok(Some((root.join(dir), file)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use system_traits::impls::RealSys;

    struct Fixture {
        _temp: tempfile::TempDir,
        root: PathBuf,
        primary: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let base = std::fs::canonicalize(temp.path()).unwrap();
            let root = base.join("linked");
            let primary = base.join("primary");
            for dir in [&root, &primary] {
                std::fs::create_dir(dir).unwrap();
                std::fs::write(dir.join("workspace.omni.yaml"), "{}").unwrap();
            }
            Self {
                _temp: temp,
                root,
                primary,
            }
        }

        fn state(&self) -> CacheResolutionState {
            CacheResolutionState::new(&self.root, None, &RealSys, |_| {
                Ok(Some(CheckoutLayout {
                    checkout_root: self.root.clone(),
                    primary_checkout_root: Some(self.primary.clone()),
                }))
            })
        }

        fn write(&self, root: &Path, content: &str) {
            std::fs::write(root.join("worktree.omni.yaml"), content).unwrap();
        }

        fn resolve(&self) -> PathBuf {
            self.state().resolve(&self.root, &RealSys).unwrap().0
        }
    }

    #[test]
    fn automatic_sharing_and_declaring_file_relative_inheritance() {
        let f = Fixture::new();
        assert_eq!(f.resolve(), f.primary.join(OMNI_CACHE_DIR));
        f.write(&f.primary, "cache:\n  dir: ../shared/cache");
        let expected = f.primary.parent().unwrap().join("shared/cache");
        assert_eq!(f.resolve(), expected);
        f.write(&f.root, "cache: {}");
        assert_eq!(f.resolve(), expected);
        f.write(&f.root, "cache:\n  dir: .omni/cache");
        assert_eq!(f.resolve(), f.root.join(OMNI_CACHE_DIR));
        f.write(&f.primary, "bad: config");
        assert_eq!(f.resolve(), f.root.join(OMNI_CACHE_DIR));
    }

    #[test]
    fn explicit_override_skips_discovery_and_invalid_configuration() {
        let f = Fixture::new();
        f.write(&f.root, "invalid: true");
        let state = CacheResolutionState::new(
            &f.root,
            Some(f.root.join("custom")),
            &RealSys,
            |_| panic!("explicit override must not discover"),
        );
        assert!(state.configuration_paths(&f.root).is_empty());
        assert_eq!(
            state.resolve(&f.root, &RealSys).unwrap().0,
            f.root.join("custom")
        );
        for path in ["", "  "] {
            let state = CacheResolutionState::new(
                &f.root,
                Some(path.into()),
                &RealSys,
                |_| panic!(),
            );
            assert!(state.resolve(&f.root, &RealSys).is_err());
        }
    }

    #[test]
    fn selected_invalid_values_and_duplicate_variants_are_errors() {
        let f = Fixture::new();
        for content in [
            "cache: {dir: ''}",
            "cache: {dir: '  '}",
            "cache: {dir: '~/cache'}",
            "cache: {dir: '$HOME/cache'}",
            "cache: {dir: '${HOME}/cache'}",
            "cache: {dir: 'state/$HOME/cache'}",
            "cache: {dir: '%LOCALAPPDATA%/cache'}",
            "cache: {directory: cache}",
            "extends: x",
            "[:",
        ] {
            f.write(&f.root, content);
            assert!(f.state().resolve(&f.root, &RealSys).is_err(), "{content}");
        }
        f.write(&f.root, "{}");
        std::fs::write(f.root.join("worktree.omni.json"), "{}").unwrap();
        assert!(f.state().resolve(&f.root, &RealSys).is_err());
    }

    #[test]
    fn null_fields_inherit_primary_configuration_or_automatic_default() {
        let f = Fixture::new();
        f.write(&f.primary, "cache: {dir: shared}");
        for content in [
            "cache: null",
            "cache:",
            "cache: {dir: null}",
            "cache:\n  dir:",
        ] {
            f.write(&f.root, content);
            assert_eq!(f.resolve(), f.primary.join("shared"));
        }
        std::fs::remove_file(f.root.join("worktree.omni.yaml")).unwrap();
        for content in [r#"{"cache":null}"#, r#"{"cache":{"dir":null}}"#] {
            std::fs::write(f.root.join("worktree.omni.json"), content).unwrap();
            assert_eq!(f.resolve(), f.primary.join("shared"));
        }
        for content in ["cache: null", "cache: {dir: null}"] {
            f.write(&f.primary, content);
            assert_eq!(f.resolve(), f.primary.join(OMNI_CACHE_DIR));
        }
        let local =
            CacheResolutionState::new(&f.root, None, &RealSys, |_| Ok(None));
        assert_eq!(
            local.resolve(&f.root, &RealSys).unwrap().0,
            f.root.join(OMNI_CACHE_DIR)
        );
    }

    #[test]
    fn every_supported_format_and_empty_yaml_loads() {
        let f = Fixture::new();
        for (name, contents) in [
            ("worktree.omni.yml", "cache: {dir: custom}"),
            ("worktree.omni.yaml", "cache: {dir: custom}"),
            ("worktree.omni.json", r#"{"cache":{"dir":"custom"}}"#),
            ("worktree.omni.toml", "[cache]\ndir = 'custom'"),
        ] {
            let path = f.root.join(name);
            std::fs::write(&path, contents).unwrap();
            assert_eq!(f.resolve(), f.root.join("custom"));
            std::fs::remove_file(path).unwrap();
        }
        for contents in ["", "  \n", "# machine-local configuration\n"] {
            f.write(&f.root, contents);
            assert_eq!(f.resolve(), f.primary.join(OMNI_CACHE_DIR));
        }
    }

    #[test]
    fn literal_paths_keep_meaningful_spaces_and_nonexpansion_characters() {
        let f = Fixture::new();
        for dir in [
            "cache directory",
            " cache ",
            "cost$5",
            "discount%",
            "tilde~cache",
        ] {
            f.write(&f.root, &format!("cache: {{dir: '{dir}'}}"));
            assert_eq!(f.resolve(), f.root.join(dir));
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_paths_use_native_absolute_semantics_and_reject_drive_relative() {
        let f = Fixture::new();
        let absolute = f.primary.join("dedicated-cache");
        f.write(
            &f.root,
            &format!("cache: {{dir: '{}'}}", absolute.display()),
        );
        assert_eq!(f.resolve(), absolute);
        f.write(&f.root, "cache: {dir: 'C:cache'}");
        assert!(f.state().resolve(&f.root, &RealSys).is_err());
    }

    #[test]
    fn unsafe_paths_are_rejected_for_all_sources() {
        let f = Fixture::new();
        for path in [&f.root, &f.primary, f.root.parent().unwrap()] {
            f.write(&f.root, &format!("cache:\n  dir: '{}'", path.display()));
            assert!(f.state().resolve(&f.root, &RealSys).is_err());
        }
        for path in [&f.root, f.root.parent().unwrap()] {
            let state = CacheResolutionState::new(
                &f.root,
                Some(path.into()),
                &RealSys,
                |_| panic!(),
            );
            assert!(state.resolve(&f.root, &RealSys).is_err());
        }
        f.write(&f.root, "cache: {dir: 'not-created/../..'}");
        assert!(f.state().resolve(&f.root, &RealSys).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn aliases_and_symlink_parent_components_keep_filesystem_meaning() {
        let f = Fixture::new();
        let target = f.primary.join("child");
        std::fs::create_dir(&target).unwrap();
        std::os::unix::fs::symlink(&target, f.root.join("alias")).unwrap();
        f.write(&f.root, "cache: {dir: 'alias/..'}");
        assert!(f.state().resolve(&f.root, &RealSys).is_err());
        f.write(&f.root, "cache: {dir: 'alias/../new/cache'}");
        assert_eq!(f.resolve(), f.primary.join("new/cache"));
    }

    #[test]
    fn resolution_retains_topology_but_rereads_file_policy() {
        let f = Fixture::new();
        let calls = std::cell::Cell::new(0);
        let state = CacheResolutionState::new(&f.root, None, &RealSys, |_| {
            calls.set(calls.get() + 1);
            Ok(Some(CheckoutLayout {
                checkout_root: f.root.clone(),
                primary_checkout_root: Some(f.primary.clone()),
            }))
        });
        assert_eq!(
            state.resolve(&f.root, &RealSys).unwrap().0,
            f.primary.join(OMNI_CACHE_DIR)
        );
        f.write(&f.primary, "cache: {dir: changed}");
        assert_eq!(
            state.resolve(&f.root, &RealSys).unwrap().0,
            f.primary.join("changed")
        );
        assert_eq!(calls.get(), 1);
        assert_eq!(state.configuration_paths(&f.root).len(), 8);
    }

    #[test]
    fn nested_workspaces_require_a_safe_corresponding_marker() {
        let f = Fixture::new();
        let nested = f.root.join("tools/docs");
        std::fs::create_dir_all(&nested).unwrap();
        let layout = CheckoutLayout {
            checkout_root: f.root.clone(),
            primary_checkout_root: Some(f.primary.clone()),
        };
        assert_eq!(mapped_workspace(&nested, layout.clone(), &RealSys), None);
        let mapped = f.primary.join("tools/docs");
        std::fs::create_dir_all(&mapped).unwrap();
        assert_eq!(mapped_workspace(&nested, layout.clone(), &RealSys), None);
        std::fs::write(mapped.join("workspace.omni.json"), "{}").unwrap();
        assert_eq!(mapped_workspace(&nested, layout, &RealSys), Some(mapped));
    }

    #[cfg(unix)]
    #[test]
    fn nested_mapping_cannot_escape_through_a_symlink() {
        let f = Fixture::new();
        let nested = f.root.join("nested");
        std::fs::create_dir(&nested).unwrap();
        std::os::unix::fs::symlink(&f.root, f.primary.join("nested")).unwrap();
        let layout = CheckoutLayout {
            checkout_root: f.root.clone(),
            primary_checkout_root: Some(f.primary.clone()),
        };
        assert_eq!(mapped_workspace(&nested, layout, &RealSys), None);
    }

    #[test]
    fn unavailable_discovery_falls_back_locally() {
        let f = Fixture::new();
        for result in [Ok(None), Err(eyre::eyre!("broken metadata"))] {
            let state =
                CacheResolutionState::new(&f.root, None, &RealSys, |_| result);
            assert_eq!(
                state.resolve(&f.root, &RealSys).unwrap().0,
                f.root.join(OMNI_CACHE_DIR)
            );
        }
    }
}
