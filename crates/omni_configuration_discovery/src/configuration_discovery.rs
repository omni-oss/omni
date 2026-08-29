use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

use derive_new::new;
use dir_walker::{
    DirEntry as _, DirWalker,
    impls::{IgnoreRealDirWalker, IgnoreRealDirWalkerConfig},
};
use omni_glob::GlobMatcher;

use crate::error::{Error, ErrorInner};

#[derive(Debug, Clone, new)]
pub struct ConfigurationDiscovery<'a, G, C, I>
where
    G: AsRef<str>,
    C: AsRef<str>,
    I: AsRef<str>,
{
    #[new(into)]
    root_dir: &'a Path,

    #[new(into)]
    glob_patterns: &'a [G],

    #[new(into)]
    config_files: &'a [C],

    #[new(into)]
    ignore_files: &'a [I],

    #[new(into)]
    config_name: &'a str,
}

impl<'a, G, C, I> ConfigurationDiscovery<'a, G, C, I>
where
    G: AsRef<str>,
    C: AsRef<str>,
    I: AsRef<str>,
{
    fn create_default_dir_walker(
        &self,
    ) -> Result<impl DirWalker + 'static, Error> {
        let cfg_builder = IgnoreRealDirWalkerConfig::builder();

        let cfg = cfg_builder
            .standard_filters(true)
            .custom_ignore_filenames(
                self.ignore_files
                    .iter()
                    .map(|s| s.as_ref().to_string())
                    .collect::<Vec<_>>(),
            )
            .build();

        Ok(IgnoreRealDirWalker::new_with_config(cfg))
    }

    pub async fn discover(&self) -> Result<Vec<PathBuf>, Error> {
        let walker = self.create_default_dir_walker()?;
        self.discover_with_walker(&walker).await
    }

    pub async fn discover_with_walker<TDirWalker: DirWalker>(
        &self,
        walker: &TDirWalker,
    ) -> Result<Vec<PathBuf>, Error> {
        let mut discovered = vec![];

        let matcher = GlobMatcher::rooted(
            self.root_dir,
            self.glob_patterns,
            Default::default(),
        )?;

        let start_walk_time = SystemTime::now();

        let mut num_iterations = 0;

        for f in walker.walk_dir(&[self.root_dir]).map_err(|e| {
            ErrorInner::new_walk_dir(self.root_dir.to_path_buf(), e)
        })? {
            num_iterations += 1;
            let f = f.map_err(ErrorInner::new_failed_to_get_dir_entry)?;
            trace::trace!(path = ?f.path(), "checking_path");

            if f.is_dir() {
                continue;
            }

            if matcher.is_match(f.path()) {
                for file_name in self.config_files {
                    if *f.file_name().to_string_lossy() == *file_name.as_ref() {
                        log::trace!(
                            "Found {} config: {:?}",
                            self.config_name,
                            f.path()
                        );
                        discovered.push(f.path().to_path_buf());
                        break;
                    }
                }
            }
        }

        log::debug!(
            "Found {} {} configs in {:?}, walked {} items",
            discovered.len(),
            self.config_name,
            start_walk_time.elapsed().unwrap_or_default(),
            num_iterations
        );

        Ok(discovered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Discovery must not descend symlinked directories. A projection can
    /// materialize a symlinked directory link into the workspace; the guardrail
    /// that leaves symlinked directory contents unchecked relies on discovery
    /// never loading a manifest through such a link. If discovery is ever
    /// changed to follow symlinks, this test fails loudly and that guardrail
    /// must be revisited.
    #[tokio::test]
    async fn discovery_does_not_follow_symlinked_directories() {
        let config_files = ["project.omni.yaml".to_string()];

        let root = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();

        // A real manifest under the root proves discovery works at all.
        std::fs::write(root.path().join("project.omni.yaml"), b"name: real\n")
            .unwrap();

        // A manifest reachable only through a symlinked directory.
        std::fs::write(
            external.path().join("project.omni.yaml"),
            b"name: hidden\n",
        )
        .unwrap();
        let link = root.path().join("linked");
        #[cfg(unix)]
        std::os::unix::fs::symlink(external.path(), &link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(external.path(), &link).unwrap();

        let ignore_files = [".omniignore".to_string()];
        let discovery = ConfigurationDiscovery::new(
            root.path(),
            &config_files[..],
            &config_files[..],
            &ignore_files[..],
            "project",
        );
        let found = discovery.discover().await.unwrap();

        assert_eq!(
            found.len(),
            1,
            "only the real manifest is discovered, not the symlinked one: {found:?}"
        );
        assert!(
            found
                .iter()
                .all(|p| !p.to_string_lossy().contains("linked")),
            "a manifest inside a symlinked directory must not be discovered: {found:?}"
        );
    }
}
