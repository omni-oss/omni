use std::path::Path;

use omni_generator_configurations::AddInlineActionConfiguration;

use crate::{
    GeneratorSys,
    action_handlers::{
        HandlerContext,
        utils::{ensure_dir_exists, get_output_path, overwrite},
    },
    error::{Error, ErrorInner},
};

pub async fn add_inline<'a>(
    config: &AddInlineActionConfiguration,
    ctx: &HandlerContext<'a>,
    sys: &impl GeneratorSys,
) -> Result<(), Error> {
    let output_path = get_output_path(
        config.base.common.target.as_deref(),
        &config.output_path,
        None,
        ctx,
        &[],
        sys,
    )
    .await?;

    let expanded_output = tera::Tera::one_off(
        &output_path.to_string_lossy(),
        ctx.tera_context_values,
        false,
    )?;
    let output_path = Path::new(&expanded_output);

    if let Some(did_overwrite) = overwrite(
        &output_path,
        ctx.overwrite.or(config.base.common.overwrite),
        sys,
    )
    .await?
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

    trace::info!("Wrote to path {}", output_path.display());

    Ok(())
}
