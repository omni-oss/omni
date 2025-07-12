use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

use async_trait::async_trait;

pub enum Script<'a> {
    Source(Cow<'a, str>),
    File(Cow<'a, Path>),
}

impl<'a> Script<'a> {
    pub fn from_file_path(path: &'a Path) -> Self {
        Self::File(Cow::Borrowed(path))
    }

    pub fn from_file_path_buf(path: PathBuf) -> Self {
        Self::File(Cow::Owned(path))
    }

    pub fn from_source_str(source: &'a str) -> Self {
        Self::Source(Cow::Borrowed(source))
    }

    pub fn from_source_string(source: String) -> Self {
        Self::Source(Cow::Owned(source))
    }
}

impl<'a> From<&'a str> for Script<'a> {
    fn from(s: &'a str) -> Self {
        Self::from_source_str(s)
    }
}

impl<'a> From<String> for Script<'a> {
    fn from(s: String) -> Self {
        Self::from_source_string(s)
    }
}

impl<'a> From<&'a Path> for Script<'a> {
    fn from(p: &'a Path) -> Self {
        Self::from_file_path(p)
    }
}

impl From<PathBuf> for Script<'_> {
    fn from(value: PathBuf) -> Self {
        Self::from_file_path_buf(value)
    }
}

#[async_trait]
pub trait JsRuntime {
    type Error;
    type ExitValue;

    async fn run<'script>(
        &mut self,
        script: Script<'script>,
        root_dir: Option<&Path>,
    ) -> Result<Self::ExitValue, Self::Error>;
}
