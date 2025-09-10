use std::path::Path;

pub use omni_context::*;

use crate::commands::CliArgs;

pub fn from_args_root_dir_and_sys<TSys: ContextSys>(
    cli: &CliArgs,
    root_dir: impl AsRef<Path>,
    sys: TSys,
) -> eyre::Result<Context<TSys>> {
    let env = cli.env.as_deref().unwrap_or("development");
    let env_files = cli
        .env_file
        .iter()
        .map(|s| {
            if s.contains("{ENV}") {
                s.replace("{ENV}", env)
            } else {
                s.to_string()
            }
        })
        .collect::<Vec<_>>();

    let root_marker = cli
        .env_root_dir_marker
        .clone()
        .unwrap_or_else(|| constants::WORKSPACE_OMNI.replace("{ext}", "yaml"));
    let ctx = Context::new(
        root_dir.as_ref(),
        cli.inherit_env_vars,
        &root_marker,
        env_files,
        sys,
    )?;

    Ok(ctx)
}

pub fn from_args_and_sys<TSys: ContextSys>(
    cli: &CliArgs,
    sys: TSys,
) -> eyre::Result<Context<TSys>> {
    let root_dir = get_root_dir(&sys)?;

    let ctx = from_args_root_dir_and_sys(cli, root_dir, sys)?;

    Ok(ctx)
}
