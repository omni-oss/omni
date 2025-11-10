use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

use derive_new::new;
use dir_walker::{
    DirEntry as _, DirWalker, Metadata as _,
    impls::{IgnoreRealDirWalker, IgnoreRealDirWalkerConfig},
};
use globset::{Glob, GlobSetBuilder};

use crate::error::{Error, ErrorInner};

#[derive(Debug, Clone, new)]
pub struct ConfigurationDiscovery<'a> {
    #[new(into)]
    root_dir: &'a Path,

    #[new(into)]
    glob_patterns: &'a [String],

    #[new(into)]
    config_files: &'a [String],

    #[new(into)]
    ignore_files: &'a [String],

    #[new(into)]
    config_name: &'a str,
}

impl<'a> ConfigurationDiscovery<'a> {
    fn create_default_dir_walker(
        &self,
    ) -> Result<impl DirWalker + 'static, Error> {
        let mut cfg_builder = IgnoreRealDirWalkerConfig::builder();

        let mut globset = GlobSetBuilder::new();

        let root = self.root_dir.to_string_lossy().to_string();
        let root = if cfg!(windows) && root.contains('\\') {
            root.replace('\\', "/")
        } else {
            root
        };

        for glob in self.glob_patterns {
            globset.add(
                Glob::new(&format!("{}/{}", root, glob))
                    .expect("can't create glob"),
            );
        }
        let matcher = globset.build()?;

        let cfg = cfg_builder
            .standard_filters(true)
            .filter_entry(move |entry| matcher.is_match(entry.path()))
            .custom_ignore_filenames(self.ignore_files.to_vec())
            .build()?;

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

        let mut match_include = GlobSetBuilder::new();

        for p in self
            .glob_patterns
            .iter()
            .filter(|p| count_starts_with(p, "!") % 2 == 0)
        {
            let pat = format!(
                "{}/{}",
                self.root_dir.display(),
                strip_starts_with(p, "!")
            );

            trace::trace!("adding include pattern: {}", pat);

            match_include.add(Glob::new(pat.as_str())?);
        }

        let include_matcher = match_include.build()?;

        let mut match_exclude = GlobSetBuilder::new();

        for p in self
            .glob_patterns
            .iter()
            .filter(|p| count_starts_with(p, "!") % 2 == 1)
        {
            let pat = format!(
                "{}/{}",
                self.root_dir.display(),
                strip_starts_with(p, "!")
            );

            trace::trace!("adding exclude pattern: {}", pat);

            match_exclude.add(Glob::new(pat.as_str())?);
        }

        let exclude_matcher = match_exclude.build()?;

        let start_walk_time = SystemTime::now();

        let mut num_iterations = 0;

        for f in walker.walk_dir(&[self.root_dir]).map_err(|e| {
            ErrorInner::new_walk_dir(self.root_dir.to_path_buf(), e)
        })? {
            num_iterations += 1;
            let f = f.map_err(ErrorInner::new_failed_to_get_dir_entry)?;
            trace::trace!("checking path: {:?}", f.path());

            let meta = f.metadata().map_err(|e| {
                ErrorInner::new_failed_to_get_metadata(
                    f.path().to_path_buf(),
                    e,
                )
            })?;

            if meta.is_dir() {
                continue;
            }

            if include_matcher.is_match(f.path())
                && !exclude_matcher.is_match(f.path())
            {
                for file_name in self.config_files {
                    if *f.file_name().to_string_lossy() == *file_name {
                        trace::trace!(
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

        trace::debug!(
            "Found {} {} configs in {:?}, walked {} items",
            discovered.len(),
            self.config_name,
            start_walk_time.elapsed().unwrap_or_default(),
            num_iterations
        );

        Ok(discovered)
    }
}

fn count_starts_with(mut s: &str, prefix: &str) -> usize {
    if prefix.is_empty() {
        return s.len();
    }

    let mut count = 0;
    let prefix_len = prefix.len();

    while let Some(pos) = s.find(prefix) {
        if pos == 0 {
            count += 1;
        }
        s = &s[pos + prefix_len..];
    }

    count
}

fn strip_starts_with<'a>(mut s: &'a str, prefix: &str) -> &'a str {
    if prefix.is_empty() {
        return s;
    }
    while let Some(stripped) = s.strip_prefix(prefix) {
        s = stripped;
    }

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_starts_with() {
        assert_eq!(count_starts_with("abc", "a"), 1);
        assert_eq!(count_starts_with("abc", "ab"), 1);
        assert_eq!(count_starts_with("abc", "abc"), 1);
        assert_eq!(count_starts_with("abc", "b"), 0);
        assert_eq!(count_starts_with("abc", "c"), 0);
        assert_eq!(count_starts_with("abc", ""), 3);
    }

    #[test]
    fn test_strip_starts_with() {
        assert_eq!(strip_starts_with("abc", "a"), "bc");
        assert_eq!(strip_starts_with("abc", "ab"), "c");
        assert_eq!(strip_starts_with("abc", "abc"), "");
        assert_eq!(strip_starts_with("abc", "b"), "abc");
        assert_eq!(strip_starts_with("abc", "c"), "abc");
        assert_eq!(strip_starts_with("abc", ""), "abc");
    }
}
