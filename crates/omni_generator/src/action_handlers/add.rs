use std::path::Path;

use omni_generator_configurations::AddActionConfiguration;

use crate::{
    GeneratorSys,
    action_handlers::{
        HandlerContext,
        utils::{ensure_dir_exists, get_output_path, overwrite},
    },
    error::{Error, ErrorInner},
};

pub async fn add<'a>(
    config: &AddActionConfiguration,
    ctx: &HandlerContext<'a>,
    sys: &impl GeneratorSys,
) -> Result<(), Error> {
    let template_file = ctx.generator_dir.join(&config.template_file);
    let file = sys
        .fs_read_async(&template_file)
        .await
        .map_err(|e| ErrorInner::new_failed_to_read_file(&template_file, e))?;
    let template_string = String::from_utf8(file.to_vec())?;

    let output_path = get_output_path(
        config.base.common.target.as_deref(),
        &config.template_file,
        config.base_path.as_deref(),
        ctx,
        &["tpl"],
        sys,
    )
    .await?;
    let expanded_output = tera::Tera::one_off(
        &output_path.to_string_lossy(),
        ctx.tera_context_values,
        false,
    )?;
    let output_path = Path::new(&expanded_output);
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
        tera::Tera::one_off(&template_string, ctx.tera_context_values, false)?;
    sys.fs_write_async(&output_path, &result)
        .await
        .map_err(|e| ErrorInner::new_failed_to_write_file(&output_path, e))?;
    trace::info!("Wrote to path {}", output_path.display());
    Ok(())
}
