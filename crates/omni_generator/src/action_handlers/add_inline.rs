use omni_generator_configurations::AddInlineActionConfiguration;

use crate::{
    GeneratorSys,
    action_handlers::{
        HandlerContext,
        utils::{ensure_dir_exists, overwrite, resolve_output_path},
    },
    error::{Error, ErrorInner},
};

pub async fn add_inline<'a>(
    config: &AddInlineActionConfiguration,
    ctx: &HandlerContext<'a>,
    sys: &impl GeneratorSys,
) -> Result<(), Error> {
    let output_path = resolve_output_path(
        ctx.output_dir,
        config
            .base
            .common
            .target
            .as_deref()
            .map(|t| ctx.targets.get(t).map(|t| t.as_path()))
            .flatten(),
        ctx.generator_dir,
        &config.output_path,
    )?;

    if let Some(did_overwrite) =
        overwrite(&output_path, config.base.common.overwrite, sys).await?
        && !did_overwrite
    {
        trace::info!("Skipped writing to path {}", output_path.display());
        return Ok(());
    }

    ensure_dir_exists(&output_path.parent().expect("should have parent"), sys)
        .await?;

    let result =
        tera::Tera::one_off(&config.template, ctx.tera_context_values, false)?;

    sys.fs_write_async(&output_path, &result)
        .await
        .map_err(|e| ErrorInner::new_failed_to_write_file(&output_path, e))?;

    Ok(())
}
