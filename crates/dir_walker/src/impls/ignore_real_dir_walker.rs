use std::{result::Result, sync::Arc};

use ignore::{WalkState, overrides::OverrideBuilder};
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use self::ignore_real_dir_walker_config_builder::{
    IsUnset, SetFilterEntry, State,
};
use crate::{DirEntry, DirWalkerBase, Metadata};

pub type Predicate =
    Arc<dyn Fn(&ignore::DirEntry) -> bool + Send + Sync + 'static>;

#[derive(Clone, bon::Builder)]
pub struct IgnoreRealDirWalkerConfig {
    #[builder(into)]
    pub standard_filters: Option<bool>,
    #[builder(into)]
    pub hidden: Option<bool>,
    #[builder(into)]
    pub ignore: Option<bool>,
    #[builder(into)]
    pub git_ignore: Option<bool>,
    #[builder(into)]
    pub git_exclude: Option<bool>,
    #[builder(into)]
    pub git_global: Option<bool>,
    #[builder(into)]
    pub ignore_case_insensitive: Option<bool>,
    #[builder(into, default)]
    pub custom_ignore_filenames: Vec<String>,
    #[builder(into)]
    pub overrides: Option<IgnoreOverridesConfig>,
    #[builder(setters(vis = "", name = filter_entry_internal))]
    pub filter_entry: Option<Predicate>,
}

impl<S: State> IgnoreRealDirWalkerConfigBuilder<S> {
    pub fn filter_entry<F>(
        self,
        filter_entry: F,
    ) -> IgnoreRealDirWalkerConfigBuilder<SetFilterEntry<S>>
    where
        F: Fn(&ignore::DirEntry) -> bool + Send + Sync + 'static,
        S::FilterEntry: IsUnset,
    {
        let x = Arc::new(filter_entry);
        self.filter_entry_internal(x)
    }
}

#[derive(Clone, Debug)]
pub struct IgnoreOverridesConfig {
    pub root: String,
    pub includes: Vec<String>,
    pub excludes: Vec<String>,
}

impl Default for IgnoreRealDirWalkerConfig {
    fn default() -> Self {
        Self {
            ignore_case_insensitive: None,
            standard_filters: Some(true),
            git_ignore: None,
            hidden: None,
            ignore: None,
            git_exclude: None,
            git_global: None,
            custom_ignore_filenames: vec![],
            overrides: None,
            filter_entry: None,
        }
    }
}

impl IgnoreRealDirWalkerConfig {
    pub(crate) fn apply<'builder>(
        &self,
        builder: &'builder mut ignore::WalkBuilder,
    ) -> Result<&'builder mut ignore::WalkBuilder, ignore::Error> {
        if let Some(conf) = &self.overrides
            && (!conf.includes.is_empty() || !conf.excludes.is_empty())
        {
            let overrides = &mut OverrideBuilder::new(&conf.root);

            if !conf.includes.is_empty() {
                for include in conf.includes.iter() {
                    overrides.add(include)?;
                }
            }

            if !conf.excludes.is_empty() {
                for exclude in conf.excludes.iter() {
                    overrides.add(&format!("!{}", exclude))?;
                }
            }

            let overrides = overrides.build()?;
            trace::trace!("added_overrides_to_ignore_builder");
            builder.overrides(overrides);
        }

        if let Some(standard_filters) = self.standard_filters {
            builder.standard_filters(standard_filters);
        }
        if let Some(ignore_case_insensitive) = self.ignore_case_insensitive {
            builder.ignore_case_insensitive(ignore_case_insensitive);
        }
        if let Some(ignore) = self.ignore {
            builder.ignore(ignore);
        }
        if let Some(git_ignore) = self.git_ignore {
            builder.git_ignore(git_ignore);
        }
        if let Some(git_global) = self.git_global {
            builder.git_global(git_global);
        }
        if let Some(git_exclude) = self.git_exclude {
            builder.git_exclude(git_exclude);
        }
        if let Some(hidden) = self.hidden {
            builder.hidden(hidden);
        }

        for ignore_filename in self.custom_ignore_filenames.iter() {
            builder.add_custom_ignore_filename(ignore_filename);
        }

        if let Some(filter) = &self.filter_entry {
            let filter = filter.clone();
            builder.filter_entry(move |p| filter(p));
        }

        Ok(builder)
    }
}

#[derive(Clone, Default)]
pub struct IgnoreRealDirWalker {
    config: IgnoreRealDirWalkerConfig,
}

impl IgnoreRealDirWalker {
    pub fn new_with_config(config: IgnoreRealDirWalkerConfig) -> Self {
        Self { config }
    }
}

impl DirWalkerBase for IgnoreRealDirWalker {
    type Error = IgnoreRealDirWalkerError;
    type IterError = ignore::Error;
    type DirEntry = IgnoreRealDirEntry;
    type WalkDir = IgnoreRealWalkDir;

    fn base_walk_dir(
        &self,
        paths: &[&std::path::Path],
    ) -> Result<IgnoreRealWalkDir, Self::Error> {
        if paths.is_empty() {
            return Err(IgnoreRealDirWalkerErrorInner::PathCantBeEmpty)?;
        }

        let mut builder = ignore::WalkBuilder::new(paths[0]);

        for p in &paths[1..] {
            builder.add(p);
        }

        let builder = self.config.apply(&mut builder)?;

        // The parallel walk spreads `getdents`/`openat` across cores, which is
        // a large win for this I/O-bound workload (~4x faster than a
        // single-threaded walk). Every walked entry is streamed to a single
        // consumer over the channel.
        let walk = builder.threads(num_cpus::get()).build_parallel();
        let (tx, rx) = crossbeam_channel::bounded(100);
        std::thread::spawn(move || {
            walk.run(|| {
                let tx = tx.clone();
                Box::new(move |entry| {
                    // ignoring any not found errors is ok
                    if let Err(err) = &entry
                        && let Some(err) = err.io_error()
                        && err.kind() == std::io::ErrorKind::NotFound
                    {
                        log::trace!("Not found error, ignoring");
                        return WalkState::Continue;
                    }

                    let result = tx.send(entry.map(IgnoreRealDirEntry));

                    if let Err(err) = result {
                        let path = err.0.as_ref().ok().map(|f| f.path());
                        log::error!(
                            "Encountered during traversal: {err}, path: {path:?}",
                        );

                        return WalkState::Quit;
                    }

                    WalkState::Continue
                })
            });
            drop(tx);
        });

        Ok(IgnoreRealWalkDir { rx })
    }
}

#[derive(Debug)]
pub struct IgnoreRealDirEntry(ignore::DirEntry);

impl Metadata for std::fs::Metadata {
    fn is_dir(&self) -> bool {
        self.is_dir()
    }
    fn is_file(&self) -> bool {
        self.is_file()
    }
}

impl DirEntry for IgnoreRealDirEntry {
    type Metadata = std::fs::Metadata;
    type Error = ignore::Error;

    fn path(&self) -> &std::path::Path {
        self.0.path()
    }

    fn into_path(self) -> std::path::PathBuf {
        self.0.into_path()
    }

    fn path_is_symlink(&self) -> bool {
        self.0.path_is_symlink()
    }

    fn metadata(&self) -> Result<Self::Metadata, Self::Error> {
        self.0.metadata()
    }

    fn file_type(&self) -> Option<crate::FileType> {
        self.0.file_type().map(|ft| {
            if ft.is_dir() {
                crate::FileType::Dir
            } else if ft.is_file() {
                crate::FileType::File
            } else if ft.is_symlink() {
                crate::FileType::Symlink
            } else {
                crate::FileType::Other
            }
        })
    }

    fn file_name(&self) -> &std::ffi::OsStr {
        self.0.file_name()
    }

    fn depth(&self) -> usize {
        self.0.depth()
    }

    #[cfg(unix)]
    fn ino(&self) -> Option<u64> {
        self.0.ino()
    }
}

pub struct IgnoreRealWalkDir {
    rx: crossbeam_channel::Receiver<Result<IgnoreRealDirEntry, ignore::Error>>,
}

impl IntoIterator for IgnoreRealWalkDir {
    type Item = Result<IgnoreRealDirEntry, ignore::Error>;

    type IntoIter = IgnoreRealWalkDirIntoIter;

    fn into_iter(self) -> Self::IntoIter {
        IgnoreRealWalkDirIntoIter {
            base: self.rx.into_iter(),
        }
    }
}

pub struct IgnoreRealWalkDirIntoIter {
    base:
        crossbeam_channel::IntoIter<Result<IgnoreRealDirEntry, ignore::Error>>,
}

impl Iterator for IgnoreRealWalkDirIntoIter {
    type Item = Result<IgnoreRealDirEntry, ignore::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.base.next()
    }
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct IgnoreRealDirWalkerError(pub(crate) IgnoreRealDirWalkerErrorInner);

impl IgnoreRealDirWalkerError {
    #[allow(unused)]
    pub fn kind(&self) -> IgnoreRealDirWalkerErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<IgnoreRealDirWalkerErrorInner>> From<T>
    for IgnoreRealDirWalkerError
{
    fn from(inner: T) -> Self {
        let inner = inner.into();
        Self(inner)
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(name(IgnoreRealDirWalkerErrorKind), vis(pub))]
pub(crate) enum IgnoreRealDirWalkerErrorInner {
    #[error("path can't be empty")]
    PathCantBeEmpty,

    #[error(transparent)]
    Ignore(#[from] ignore::Error),
}
