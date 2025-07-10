use std::{
    borrow::Cow,
    hash::{DefaultHasher, Hash as _, Hasher as _},
    path::{Path, PathBuf},
    process::Stdio,
};

use async_trait::async_trait;
use futures::{AsyncRead, AsyncWrite};
use system_traits::auto_impl;

use crate::{JsRuntimeError, JsRuntimeSys, error};

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

#[auto_impl]
pub trait DelegatingJsRuntimeTransport:
    AsyncRead + AsyncWrite + Send + Sync
{
}

#[derive(Debug)]
pub struct DelegatingJsRuntime<TSys: JsRuntimeSys> {
    sys: TSys,
}

impl<TSys: JsRuntimeSys> DelegatingJsRuntime<TSys> {
    pub fn new(sys: TSys) -> Self {
        Self { sys }
    }
}

#[async_trait]
impl<TSys: JsRuntimeSys> JsRuntime for DelegatingJsRuntime<TSys> {
    type Error = JsRuntimeError;

    type ExitValue = ();

    async fn run<'script>(
        &mut self,
        script: Script<'script>,
        root_dir: Option<&Path>,
    ) -> Result<Self::ExitValue, Self::Error> {
        match script {
            Script::Source(cow) => {
                run_source_code(cow.as_ref(), root_dir, self.sys.clone()).await
            }
            Script::File(cow) => run_script(cow.as_ref(), root_dir).await,
        }
    }
}

async fn create_temp_source_file<TSys>(
    code: &str,
    root_dir: Option<&Path>,
    sys: TSys,
) -> Result<PathBuf, JsRuntimeError>
where
    TSys: JsRuntimeSys,
{
    let temp_dir = if let Some(root_dir) = root_dir {
        root_dir.join("./.omni/tmp")
    } else {
        sys.env_current_dir_async()
            .await
            .expect("Failed to get current directory")
            .join("./.omni/tmp")
    };

    if !sys.fs_exists_async(&temp_dir).await? {
        sys.fs_create_dir_all_async(&temp_dir).await.map_err(|e| {
            error::error!("Failed to create temp directory: {e}")
        })?;
    }

    let mut hasher = DefaultHasher::new();
    code.hash(&mut hasher);
    let file_name = format!("{}.js", hasher.finish());
    let temp_file = temp_dir.join(file_name);

    if !sys.fs_exists_async(&temp_file).await? {
        sys.fs_write_async(&temp_file, code)
            .await
            .map_err(|e| error::error!("Failed to write temp file: {e}"))?;
    }

    Ok(temp_file)
}

async fn run_source_code<TSys>(
    code: &str,
    root_dir: Option<&Path>,
    sys: TSys,
) -> Result<(), JsRuntimeError>
where
    TSys: JsRuntimeSys,
{
    let temp_file =
        create_temp_source_file(code, root_dir, sys.clone()).await?;

    run_script(&temp_file, root_dir).await
}

async fn run_script(
    main_module: &Path,
    root_dir: Option<&Path>,
) -> Result<(), JsRuntimeError> {
    let mut cmd = tokio::process::Command::new("deno");

    cmd.arg("run");
    cmd.arg(main_module);

    if let Some(root_dir) = root_dir {
        cmd.current_dir(root_dir);
    }

    cmd.stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| {
            let err = error::error!("Failed to spawn deno process: {e}");
            JsRuntimeError::from(err)
        })?
        .wait()
        .await
        .map_err(|e| {
            let err = error::error!("Failed to wait for deno process: {e}");
            JsRuntimeError::from(err)
        })?;

    Ok(())
}
