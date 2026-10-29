use std::path::{Path, PathBuf};

pub use omni_context::*;
use omni_tracing_subscriber::TracingConfig;

use crate::commands::CliArgs;

#[allow(clippy::result_large_err)]
pub fn from_args_root_dir_and_sys<TSys: ContextSys>(
    cli: &CliArgs,
    root_dir: impl AsRef<Path>,
    sys: TSys,
    tracing: &TracingConfig,
) -> Result<Context<TSys>, ContextError> {
    from_config_root_dir_and_sys(
        &workspace_init_config(cli),
        root_dir,
        sys,
        tracing,
    )
}

/// Extract server-wide workspace initialization policy without loading a workspace.
pub fn workspace_init_config(cli: &CliArgs) -> WorkspaceInitConfig {
    let env = cli.env.as_deref().unwrap_or("development");
    let env_files = cli.env_file.as_ref().map(|v| {
        v.iter()
            .map(|s| {
                PathBuf::from(if s.contains("{ENV}") {
                    s.replace("{ENV}", env)
                } else {
                    s.to_string()
                })
            })
            .collect::<Vec<_>>()
    });

    let root_marker = cli
        .env_root_dir_marker
        .clone()
        .unwrap_or_else(|| constants::WORKSPACE_OMNI.replace("{ext}", "yaml"));
    WorkspaceInitConfig {
        env: env.to_owned(),
        env_files,
        env_root_dir_marker: root_marker,
        inherit_env_vars: cli.inherit_env_vars,
        cache_dir: cli.cache_dir.clone(),
    }
}

#[allow(clippy::result_large_err)]
pub fn from_args_and_sys<TSys: ContextSys>(
    cli: &CliArgs,
    sys: TSys,
    tracing: &TracingConfig,
) -> Result<Context<TSys>, ContextError> {
    let root_dir = get_root_dir(&sys)?;

    let ctx = from_args_root_dir_and_sys(cli, root_dir, sys, tracing)?;

    Ok(ctx)
}

/// Context creation from WorkspaceInitConfig — use this in non-CLI entrypoints.
#[allow(clippy::result_large_err)]
pub fn from_config_root_dir_and_sys<TSys: ContextSys>(
    cfg: &WorkspaceInitConfig,
    root_dir: impl AsRef<Path>,
    sys: TSys,
    tracing: &TracingConfig,
) -> Result<Context<TSys>, ContextError> {
    let ctx = Context::new_with_cache_dir(
        sys,
        &cfg.env,
        root_dir.as_ref(),
        cfg.inherit_env_vars,
        &cfg.env_root_dir_marker,
        cfg.env_files.clone(),
        tracing,
        cfg.cache_dir.clone(),
    )?;
    Ok(ctx)
}

#[allow(clippy::result_large_err)]
pub fn from_config_and_sys<TSys: ContextSys>(
    cfg: &WorkspaceInitConfig,
    sys: TSys,
    tracing: &TracingConfig,
) -> Result<Context<TSys>, ContextError> {
    let root_dir = get_root_dir(&sys)?;
    let ctx = from_config_root_dir_and_sys(cfg, root_dir, sys, tracing)?;
    Ok(ctx)
}
