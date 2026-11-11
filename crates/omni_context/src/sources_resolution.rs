use std::path::{Path, PathBuf};

use omni_constants::{
    OMNI_SOURCES_DIR, STORE_SEGMENT, WORKTREE_OMNI, config_file_names,
};
use omni_scm::CheckoutLayout;

use crate::{
    ContextSys,
    cache_resolution::{
        WorktreeDir, configured_worktree_directory, empty_path,
        mapped_workspace,
    },
    context::{ContextError, ContextErrorInner, canonicalize_cache_dir},
};

/// Where the remote-source store lives and why.
#[derive(Clone, Debug, PartialEq)]
pub enum SourcesDirectorySource {
    Explicit,
    WorktreeFile(PathBuf),
    PrimaryFile(PathBuf),
    /// The per-user default (the platform cache directory).
    GlobalDefault,
    /// No per-user directory exists on this system: the old per-workspace
    /// store under `.omni/sources/store`.
    LocalFallback,
}

/// Construction inputs retained across reloads, mirroring
/// `CacheResolutionState`: discovery runs once, file policy is re-read.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SourcesResolutionState {
    explicit: Option<PathBuf>,
    primary: Option<PathBuf>,
    global_default: Option<PathBuf>,
}

/// The per-user default store location, `<platform cache dir>/sources`.
pub(crate) fn global_sources_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "omni-oss", "omni")
        .map(|dirs| dirs.cache_dir().join("sources"))
}

impl SourcesResolutionState {
    pub(crate) fn new(
        root: &Path,
        explicit: Option<PathBuf>,
        global_default: Option<PathBuf>,
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
                global_default,
            };
        }
        let primary = match discover(root) {
            Ok(Some(layout)) => mapped_workspace(root, layout, sys),
            Ok(None) => None,
            Err(error) => {
                tracing::debug!(
                    "Checkout discovery failed; using local sources policy: {error:#}"
                );
                None
            }
        };
        Self {
            explicit: None,
            primary,
            global_default,
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

    /// Precedence: explicit, this worktree's `sources.dir`, the primary
    /// worktree's `sources.dir`, the per-user default, then the old
    /// per-workspace store.
    #[allow(clippy::result_large_err)]
    pub(crate) fn resolve(
        &self,
        root: &Path,
        sys: &impl ContextSys,
    ) -> Result<(PathBuf, SourcesDirectorySource), ContextError> {
        let configured = |dir: &Path| {
            configured_worktree_directory(dir, sys, WorktreeDir::Sources)
        };
        let (path, source) = if let Some(path) = &self.explicit {
            if empty_path(path) {
                return Err(ContextErrorInner::SourcesDirectory(
                    "explicit sources directory must be a nonempty path".into(),
                )
                .into());
            }
            (path.clone(), SourcesDirectorySource::Explicit)
        } else if let Some((path, file)) = configured(root)? {
            (path, SourcesDirectorySource::WorktreeFile(file))
        } else if let Some((path, file)) = self
            .primary
            .as_deref()
            .filter(|primary| *primary != root)
            .map(configured)
            .transpose()?
            .flatten()
        {
            (path, SourcesDirectorySource::PrimaryFile(file))
        } else if let Some(default) = &self.global_default {
            (default.clone(), SourcesDirectorySource::GlobalDefault)
        } else {
            (
                root.join(OMNI_SOURCES_DIR).join(STORE_SEGMENT),
                SourcesDirectorySource::LocalFallback,
            )
        };
        let path = canonicalize_cache_dir(&path, sys);
        if source != SourcesDirectorySource::LocalFallback {
            for workspace in
                std::iter::once(root).chain(self.primary.as_deref())
            {
                if workspace.starts_with(&path) {
                    return Err(ContextErrorInner::SourcesDirectory(format!(
                        "unsafe sources directory {} ({source:?}): must not equal or contain workspace {}; choose a dedicated directory",
                        path.display(),
                        workspace.display()
                    ))
                    .into());
                }
            }
        }
        tracing::debug!(
            "Sources directory {} selected from {source:?}",
            path.display()
        );
        Ok((path, source))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use system_traits::impls::RealSys;

    struct Fixture {
        _temp: tempfile::TempDir,
        root: PathBuf,
        primary: PathBuf,
        global: PathBuf,
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
            let global = base.join("home/.cache/omni/sources");
            Self {
                _temp: temp,
                root,
                primary,
                global,
            }
        }

        fn state(&self, global: Option<PathBuf>) -> SourcesResolutionState {
            SourcesResolutionState::new(
                &self.root,
                None,
                global,
                &RealSys,
                |_| {
                    Ok(Some(CheckoutLayout {
                        checkout_root: self.root.clone(),
                        primary_checkout_root: Some(self.primary.clone()),
                    }))
                },
            )
        }

        fn write(&self, dir: &Path, content: &str) {
            std::fs::write(dir.join("worktree.omni.yaml"), content).unwrap();
        }

        fn resolve(&self) -> (PathBuf, SourcesDirectorySource) {
            self.state(Some(self.global.clone()))
                .resolve(&self.root, &RealSys)
                .unwrap()
        }
    }

    #[test]
    fn defaults_to_the_global_dir_for_linked_and_unrelated_workspaces() {
        let f = Fixture::new();
        assert_eq!(
            f.resolve(),
            (f.global.clone(), SourcesDirectorySource::GlobalDefault)
        );
        let local = SourcesResolutionState::new(
            &f.root,
            None,
            Some(f.global.clone()),
            &RealSys,
            |_| Ok(None),
        );
        assert_eq!(local.resolve(&f.root, &RealSys).unwrap().0, f.global);
    }

    #[test]
    fn precedence_is_worktree_then_primary_then_global() {
        let f = Fixture::new();
        f.write(&f.primary, "sources:\n  dir: ../shared/sources");
        let shared = f.primary.parent().unwrap().join("shared/sources");
        let (path, source) = f.resolve();
        assert_eq!(path, shared);
        assert!(matches!(source, SourcesDirectorySource::PrimaryFile(_)));

        f.write(&f.root, "sources:\n  dir: local-sources");
        let (path, source) = f.resolve();
        assert_eq!(path, f.root.join("local-sources"));
        assert!(matches!(source, SourcesDirectorySource::WorktreeFile(_)));

        // A null or empty worktree value inherits the primary's.
        for content in ["sources: {}", "sources: null", "sources: {dir: null}"]
        {
            f.write(&f.root, content);
            assert_eq!(f.resolve().0, shared);
        }
    }

    #[test]
    fn cache_dir_in_the_same_file_does_not_affect_sources() {
        let f = Fixture::new();
        f.write(&f.root, "cache:\n  dir: elsewhere");
        assert_eq!(f.resolve().1, SourcesDirectorySource::GlobalDefault);
    }

    #[test]
    fn explicit_wins_and_skips_discovery_and_invalid_files() {
        let f = Fixture::new();
        f.write(&f.root, "invalid: true");
        let state = SourcesResolutionState::new(
            &f.root,
            Some(f.root.join("custom")),
            Some(f.global.clone()),
            &RealSys,
            |_| panic!("explicit override must not discover"),
        );
        assert!(state.configuration_paths(&f.root).is_empty());
        assert_eq!(
            state.resolve(&f.root, &RealSys).unwrap(),
            (f.root.join("custom"), SourcesDirectorySource::Explicit)
        );
        let empty = SourcesResolutionState::new(
            &f.root,
            Some("  ".into()),
            None,
            &RealSys,
            |_| panic!(),
        );
        assert!(empty.resolve(&f.root, &RealSys).is_err());
    }

    #[test]
    fn invalid_values_are_errors_not_silently_ignored() {
        let f = Fixture::new();
        for content in [
            "sources: {dir: ''}",
            "sources: {dir: '~/s'}",
            "sources: {dir: '$HOME/s'}",
            "sources: {dir: '%LOCALAPPDATA%/s'}",
            "sources: {directory: s}",
            "[:",
        ] {
            f.write(&f.root, content);
            assert!(
                f.state(Some(f.global.clone()))
                    .resolve(&f.root, &RealSys)
                    .is_err(),
                "{content}"
            );
        }
    }

    #[test]
    fn unsafe_locations_are_rejected() {
        let f = Fixture::new();
        for path in [&f.root, &f.primary, f.root.parent().unwrap()] {
            f.write(&f.root, &format!("sources:\n  dir: '{}'", path.display()));
            assert!(
                f.state(Some(f.global.clone()))
                    .resolve(&f.root, &RealSys)
                    .is_err()
            );
        }
    }

    #[test]
    fn without_a_user_directory_it_falls_back_to_the_workspace_store() {
        let f = Fixture::new();
        let (path, source) = f.state(None).resolve(&f.root, &RealSys).unwrap();
        assert_eq!(path, f.root.join(".omni/sources/store"));
        assert_eq!(source, SourcesDirectorySource::LocalFallback);
    }

    #[test]
    fn file_policy_is_reread_but_topology_is_retained() {
        let f = Fixture::new();
        let calls = std::cell::Cell::new(0);
        let state = SourcesResolutionState::new(
            &f.root,
            None,
            Some(f.global.clone()),
            &RealSys,
            |_| {
                calls.set(calls.get() + 1);
                Ok(Some(CheckoutLayout {
                    checkout_root: f.root.clone(),
                    primary_checkout_root: Some(f.primary.clone()),
                }))
            },
        );
        assert_eq!(state.resolve(&f.root, &RealSys).unwrap().0, f.global);
        f.write(&f.primary, "sources: {dir: changed}");
        assert_eq!(
            state.resolve(&f.root, &RealSys).unwrap().0,
            f.primary.join("changed")
        );
        assert_eq!(calls.get(), 1);
    }
}
