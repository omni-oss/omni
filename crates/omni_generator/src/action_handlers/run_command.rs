use std::path::PathBuf;

use omni_generator_configurations::RunCommandActionConfiguration;
use omni_process::ChildProcess;

use crate::{
    GeneratorSys,
    action_handlers::{
        HandlerContext,
        utils::{get_bases, get_target_dir},
    },
    error::{Error, ErrorInner},
};

pub async fn run_command<'a>(
    config: &RunCommandActionConfiguration,
    ctx: &HandlerContext<'a>,
    sys: &impl GeneratorSys,
) -> Result<(), Error> {
    let bases = get_bases(ctx);

    let target = if let Some(target_name) = config.target.as_ref() {
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

    trace::info!("Running command: {}", config.command);

    if config.supports_dry_run || !ctx.dry_run {
        let mut cp = ChildProcess::<String, PathBuf>::new(
            config.command.clone(),
            target,
        );

        cp.env_vars(ctx.env)
            .keep_stdin_open(false)
            .record_logs(false);

        if config.show_output {
            cp.output_writer(tokio::io::stdout());
        }

        let result = cp.exec().await?;

        if result.exit_code() != 0 {
            return Err(ErrorInner::CommandFailed {
                command: config.command.clone(),
                exit_code: result.exit_code(),
            })?;
        }
    }

    Ok(())
}
