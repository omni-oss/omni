use std::{borrow::Cow, path::Path};

use omni_generator_configurations::CommonAddConfiguration;

use crate::{
    GeneratorSys,
    action_handlers::{
        HandlerContext,
        utils::{
            augment_tera_context, ensure_dir_exists, get_output_path, overwrite,
        },
    },
    error::{Error, ErrorInner},
};

pub async fn add_one<'a, TRender, TSys>(
    file: &'a Path,
    content: &'a str,
    base_path: Option<&'a Path>,
    render: TRender,
    common: &CommonAddConfiguration,
    flatten: bool,
    ctx: &HandlerContext<'a>,
    sys: &'a TSys,
) -> Result<(), Error>
where
    TRender:
        FnOnce(&str, &tera::Context) -> tera::Result<String> + Send + Sync + 'a,
    TSys: GeneratorSys,
{
    let output_path = get_output_path(
        common.target.as_deref(),
        &file,
        base_path,
        ctx,
        if common.render { &["tpl"] } else { &[] },
        flatten,
        ctx.gen_session,
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

    let tera_ctx_with_data =
        augment_tera_context(ctx.tera_context_values, Some(&common.data))?;

    let result = if common.render {
        Cow::Owned(render(content, &tera_ctx_with_data)?)
    } else {
        Cow::Borrowed(content)
    };

    sys.fs_write_async(&output_path, result.as_bytes())
        .await
        .map_err(|e| ErrorInner::new_failed_to_write_file(&output_path, e))?;

    trace::info!("Wrote to path {}", output_path.display());

    Ok(())
}
