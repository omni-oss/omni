use std::result::Result;

use derive_builder::Builder;
use ignore::overrides::OverrideBuilder;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::{DirEntry, DirWalkerBase, Metadata};

#[derive(Clone, Debug, Builder)]
pub struct IgnoreRealDirWalkerConfig {
    pub standard_filters: bool,
    pub custom_ignore_filenames: Vec<String>,
    pub overrides: Option<IgnoreOverridesConfig>,
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
            trace::debug!(
                config = ?conf,
                "added overrides to ignore builder",
            );
            builder.overrides(overrides);
        }
        builder.standard_filters(self.standard_filters);
        for ignore_filename in self.custom_ignore_filenames.iter() {
            builder.add_custom_ignore_filename(ignore_filename);
        }

        Ok(builder)
    }
}

#[derive(Clone, Debug, Default)]
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

        let walk = builder.build();

        Ok(IgnoreRealWalkDir { walk })
    }
}

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
    walk: ignore::Walk,
}

impl IntoIterator for IgnoreRealWalkDir {
    type Item = Result<IgnoreRealDirEntry, ignore::Error>;

    type IntoIter = IgnoreRealWalkDirIntoIter;

    fn into_iter(self) -> Self::IntoIter {
        IgnoreRealWalkDirIntoIter { walk: self.walk }
    }
}

pub struct IgnoreRealWalkDirIntoIter {
    walk: ignore::Walk,
}

impl Iterator for IgnoreRealWalkDirIntoIter {
    type Item = Result<IgnoreRealDirEntry, ignore::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.walk.next().map(|e| e.map(IgnoreRealDirEntry))
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
