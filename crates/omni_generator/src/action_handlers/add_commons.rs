use std::path::Path;

use omni_generator_configurations::CommonAddConfiguration;

use crate::{
    GeneratorSys,
    action_handlers::{
        HandlerContext,
        utils::{ensure_dir_exists, get_output_path, overwrite},
    },
    error::{Error, ErrorInner},
};

pub async fn add_one<'a, TRender, TSys>(
    template_file: &'a Path,
    base_path: Option<&'a Path>,
    render: TRender,
    common: &CommonAddConfiguration,
    ctx: &HandlerContext<'a>,
    sys: &'a TSys,
) -> Result<(), Error>
where
    TRender: FnOnce(&tera::Context) -> tera::Result<String> + Send + Sync + 'a,
    TSys: GeneratorSys,
{
    let output_path = get_output_path(
        common.target.as_deref(),
        &template_file,
        base_path,
        ctx,
        &["tpl"],
        sys,
    )
    .await?;

    let expanded_output = omni_tera::one_off(
        &output_path.to_string_lossy(),
        "output_path",
        ctx.tera_context_values,
    )?;

    let output_path = Path::new(&expanded_output);
    if let Some(did_overwrite) =
        overwrite(&output_path, ctx.overwrite.or(common.overwrite), sys).await?
        && !did_overwrite
    {
        trace::info!("Skipped writing to path {}", output_path.display());
        return Ok(());
    }

    ensure_dir_exists(&output_path.parent().expect("should have parent"), sys)
        .await?;

    let result = render(ctx.tera_context_values)?;

    sys.fs_write_async(&output_path, &result)
        .await
        .map_err(|e| ErrorInner::new_failed_to_write_file(&output_path, e))?;

    trace::info!("Wrote to path {}", output_path.display());

    Ok(())
}
