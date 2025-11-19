use std::path::PathBuf;

use env::CommandExpansionConfig;
use maps::Map;
use omni_process::ChildProcess;

use crate::{
    GeneratorSys,
    action_handlers::{
        HandlerContext,
        utils::{get_bases, get_target_dir},
    },
    error::{Error, ErrorInner},
};

pub async fn run_custom_commons<'a>(
    command: &str,
    target_name: Option<&str>,
    supports_dry_run: bool,
    override_envs: &Map<String, String>,
    show_output: bool,
    ctx: &HandlerContext<'a>,
    sys: &impl GeneratorSys,
) -> Result<(), Error> {
    let bases = get_bases(ctx);

    let target = if let Some(target_name) = target_name {
        let target = get_target_dir(
            target_name,
            ctx.target_overrides,
            ctx.generator_targets,
            ctx.output_path,
            sys,
        )
        .await?;

        ctx.output_path.join(target.as_ref().resolve(&bases))
    } else {
        ctx.output_path.to_path_buf()
    };

    let command = omni_tera::one_off(
        &command,
        format!("command for {}", ctx.resolved_action_name),
        &ctx.tera_context_values,
    )?;

    trace::info!("Running command: {}", command);

    if supports_dry_run || !ctx.dry_run {
        let mut cp = ChildProcess::<String, PathBuf>::new(
            command.clone(),
            target.clone(),
        );

        let mut expanded_env;
        let env = if !override_envs.is_empty() {
            expanded_env = ctx.env.clone();

            for (key, value) in override_envs.iter() {
                let expanded = omni_tera::one_off(
                    value,
                    format!("env value for {}", ctx.resolved_action_name),
                    &ctx.tera_context_values,
                )?;

                expanded_env.insert(key.clone(), expanded);
            }

            let vars_os = omni_utils::env::to_vars_os(&ctx.env);

            env::expand_into_with_command_config(
                &mut expanded_env,
                &ctx.env,
                &CommandExpansionConfig::new_enabled(
                    target.as_path(),
                    &vars_os,
                ),
            )?;

            &expanded_env
        } else {
            ctx.env
        };

        cp.env_vars(env).keep_stdin_open(false).record_logs(false);

        if show_output {
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
