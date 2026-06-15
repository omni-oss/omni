use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

use env::CommandExpansionConfig;
use maps::Map;
use omni_generator_configurations::CommonRunCustomActionConfiguration;
use omni_process::ChildProcess;

use crate::{
    GeneratorSys,
    action_handlers::{
        HandlerContext,
        utils::{get_bases, get_target_dir},
    },
    error::{Error, ErrorInner},
    gen_session::GenSession,
};

pub async fn target_path(
    common: &CommonRunCustomActionConfiguration,
    ctx: &HandlerContext<'_>,
    session: &GenSession,
    sys: &impl GeneratorSys,
) -> Result<PathBuf, Error> {
    let bases = get_bases(ctx);
    let result = if let Some(target_name) = common.target.as_deref() {
        let target = get_target_dir(
            target_name,
            ctx.target_overrides,
            ctx.generator_targets,
            ctx.output_dir,
            ctx.generator_name,
            session,
            ctx.input_provider,
            sys,
        )
        .await?;

        ctx.output_dir.join(target.as_ref().resolve(&bases))
    } else {
        ctx.output_dir.to_path_buf()
    };

    Ok(result)
}

/// Builds the fully-expanded environment for a custom command/script execution
/// running with `target` as its working directory.
///
/// When the action declares no extra environment variables the caller's
/// environment is returned untouched (borrowed); otherwise the declared values
/// are tera-rendered and command-expanded on top of it.
pub(crate) fn build_command_env<'a>(
    common: &CommonRunCustomActionConfiguration,
    ctx: &HandlerContext<'a>,
    target: &Path,
) -> Result<Cow<'a, Map<String, String>>, Error> {
    if common.env.is_empty() {
        return Ok(Cow::Borrowed(ctx.env));
    }

    let mut expanded_env = ctx.env.clone();

    for (key, value) in common.env.iter() {
        let expanded = omni_tera::one_off(
            value,
            format!("env value for {}", ctx.resolved_action_name),
            ctx.tera_context_values,
        )?;

        expanded_env.insert(key.clone(), expanded);
    }

    let vars_os = omni_utils::env::to_vars_os(ctx.env);

    env::expand_into_with_command_config(
        &mut expanded_env,
        ctx.env,
        &CommandExpansionConfig::new_enabled(target, &vars_os),
    )?;

    Ok(Cow::Owned(expanded_env))
}

pub async fn run_custom_commons<'a>(
    command: &str,
    target_path: Option<&Path>,
    common: &CommonRunCustomActionConfiguration,
    ctx: &HandlerContext<'a>,
    sys: &impl GeneratorSys,
) -> Result<(), Error> {
    let __s_target;
    let target = if let Some(target_path) = target_path {
        target_path
    } else {
        __s_target =
            self::target_path(common, ctx, ctx.gen_session, sys).await?;

        &__s_target
    };

    let command = omni_tera::one_off(
        &command,
        format!("command for {}", ctx.resolved_action_name),
        &ctx.tera_context_values,
    )?;

    log::info!("Running command: {}", command);

    if common.supports_dry_run || !ctx.dry_run {
        let mut cp =
            ChildProcess::<String, PathBuf>::new(command.clone(), target);

        let env = build_command_env(common, ctx, target)?;

        cp.env_vars(env.as_ref())
            .keep_stdin_open(false)
            .record_logs(false);

        if common.show_output {
            cp.output_writer(tokio::io::stdout());
        }

        let result = cp.exec().await?;

        if result.exit_code() != 0 {
            return Err(ErrorInner::CommandFailed {
                command,
                exit_code: result.exit_code(),
            })?;
        }
    }

    Ok(())
}
