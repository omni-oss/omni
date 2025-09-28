use std::{result::Result, sync::Arc};

use derive_builder::Builder;
use ignore::{WalkState, overrides::OverrideBuilder};
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::{DirEntry, DirWalkerBase, Metadata};

pub type Predicate =
    Arc<dyn Fn(&ignore::DirEntry) -> bool + Send + Sync + 'static>;

#[derive(Clone, Builder)]
#[builder(setter(into, strip_option))]
pub struct IgnoreRealDirWalkerConfig {
    pub standard_filters: bool,
    pub custom_ignore_filenames: Vec<String>,
    #[builder(default)]
    pub overrides: Option<IgnoreOverridesConfig>,
    #[builder(setter(custom), default)]
    pub filter_entry: Option<Predicate>,
}

impl IgnoreRealDirWalkerConfigBuilder {
    pub fn filter_entry<F>(&mut self, filter_entry: F) -> &mut Self
    where
        F: Fn(&ignore::DirEntry) -> bool + Send + Sync + 'static,
    {
        self.filter_entry = Some(Some(Arc::new(filter_entry)));
        self
    }
}

impl IgnoreRealDirWalkerConfig {
    pub fn builder() -> IgnoreRealDirWalkerConfigBuilder {
        IgnoreRealDirWalkerConfigBuilder::default()
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
            standard_filters: true,
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
            trace::trace!("added overrides to ignore builder");
            builder.overrides(overrides);
        }
        builder.standard_filters(self.standard_filters);
        for ignore_filename in self.custom_ignore_filenames.iter() {
            builder.add_custom_ignore_filename(ignore_filename);
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

        let walk = builder.threads(num_cpus::get()).build_parallel();
        let (tx, rx) = crossbeam_channel::bounded(100);
        std::thread::spawn(move || {
            walk.run(|| {
                let tx = tx.clone();
                Box::new(move |e| {
                    tx.send(e.map(IgnoreRealDirEntry)).unwrap();
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
#[error("{inner}")]
pub struct IgnoreRealDirWalkerError {
    kind: IgnoreRealDirWalkerErrorKind,
    #[source]
    inner: IgnoreRealDirWalkerErrorInner,
}

impl<T: Into<IgnoreRealDirWalkerErrorInner>> From<T>
    for IgnoreRealDirWalkerError
{
    fn from(inner: T) -> Self {
        let inner = inner.into();
        let kind = inner.discriminant();
        Self { inner, kind }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(name(IgnoreRealDirWalkerErrorKind), vis(pub))]
enum IgnoreRealDirWalkerErrorInner {
    #[error("path can't be empty")]
    PathCantBeEmpty,

    #[error(transparent)]
    Ignore(#[from] ignore::Error),
}
