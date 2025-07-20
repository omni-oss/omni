use std::{
    hash::{DefaultHasher, Hash as _, Hasher as _},
    path::{Path, PathBuf},
    process::Stdio,
};

use async_trait::async_trait;
use futures::{AsyncRead, AsyncWrite};
use system_traits::auto_impl;
use tokio::process::Command;

use crate::{BaseJsRuntime, JsRuntimeError, JsRuntimeSys, Script, error};

#[auto_impl]
pub trait DelegatingJsRuntimeTransport:
    AsyncRead + AsyncWrite + Send + Sync
{
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelegatingJsRuntimeOption {
    Deno,
    Node,
    Bun,
    Auto,
}

#[derive(Debug)]
pub struct DelegatingJsRuntime<TSys: JsRuntimeSys> {
    sys: TSys,
    runtime: DelegatingJsRuntimeOption,
}

impl<TSys: JsRuntimeSys> DelegatingJsRuntime<TSys> {
    pub fn new(sys: TSys, runtime: DelegatingJsRuntimeOption) -> Self {
        Self { sys, runtime }
    }
}

#[async_trait]
impl<TSys: JsRuntimeSys> BaseJsRuntime for DelegatingJsRuntime<TSys> {
    type Error = JsRuntimeError;

    type ExitValue = ();

    async fn base_run<'script>(
        &mut self,
        script: Script<'script>,
        root_dir: Option<&Path>,
    ) -> Result<Self::ExitValue, Self::Error> {
        match script {
            Script::Source(cow) => {
                run_source_code(
                    cow.as_ref(),
                    root_dir,
                    self.sys.clone(),
                    self.runtime,
                )
                .await
            }
            Script::File(cow) => {
                run_script(cow.as_ref(), root_dir, self.runtime).await
            }
        }
    }
}

async fn create_temp_source_file<TSys>(
    code: &str,
    root_dir: Option<&Path>,
    sys: TSys,
    is_ts: bool,
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
    let file_name =
        format!("{}.{}", hasher.finish(), if is_ts { "ts" } else { "js" });
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
    rt_option: DelegatingJsRuntimeOption,
) -> Result<(), JsRuntimeError>
where
    TSys: JsRuntimeSys,
{
    let temp_file =
        create_temp_source_file(code, root_dir, sys.clone(), true).await?;

    run_script(&temp_file, root_dir, rt_option).await
}

fn auto_detect_runtime_option() -> Option<DelegatingJsRuntimeOption> {
    Some(if which::which("deno").is_ok() {
        DelegatingJsRuntimeOption::Deno
    } else if which::which("node").is_ok() {
        DelegatingJsRuntimeOption::Node
    } else if which::which("bun").is_ok() {
        DelegatingJsRuntimeOption::Bun
    } else {
        return None;
    })
}

fn build_cmd(
    runtime: DelegatingJsRuntimeOption,
    main_module: &Path,
) -> Command {
    let rt = if runtime == DelegatingJsRuntimeOption::Auto {
        auto_detect_runtime_option().expect("Can't auto detect runtime")
    } else {
        runtime
    };

    let mut cmd = match rt {
        DelegatingJsRuntimeOption::Deno => Command::new("deno"),
        DelegatingJsRuntimeOption::Node => Command::new("node"),
        DelegatingJsRuntimeOption::Bun => Command::new("bun"),
        DelegatingJsRuntimeOption::Auto => {
            unreachable!("Auto select runtime should be unreachable")
        }
    };

    if rt != DelegatingJsRuntimeOption::Node {
        cmd.arg("run");
    }

    if rt == DelegatingJsRuntimeOption::Deno {
        cmd.arg("--allow-all");
    }

    cmd.arg(main_module);
    cmd
}

async fn run_script(
    main_module: &Path,
    root_dir: Option<&Path>,
    rt_option: DelegatingJsRuntimeOption,
) -> Result<(), JsRuntimeError> {
    let mut cmd = build_cmd(rt_option, main_module);

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
