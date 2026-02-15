use std::{borrow::Cow, path::Path};

use omni_generator_configurations::CommonModifyConfiguration;

use crate::{
    GeneratorSys,
    action_handlers::{
        HandlerContext,
        utils::{augment_tera_context, get_target_file},
    },
    error::{Error, ErrorInner},
};

pub async fn modify_one<'a>(
    template: &'a str,
    common: &'a CommonModifyConfiguration,
    ctx: &HandlerContext<'a>,
    sys: &'a impl GeneratorSys,
) -> Result<(), Error> {
    let rg = regex::Regex::new(&common.pattern)?;
    let target_name = &common.target;
    let target = get_target_file(target_name, ctx, sys).await?;

    let tera_ctx_with_data =
        augment_tera_context(ctx.tera_context_values, Some(&common.data))?;

    let target = omni_tera::one_off(
        &target.to_string_lossy(),
        "output_path",
        &tera_ctx_with_data,
    )?;

    let content = sys.fs_read_to_string_async(&target).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ErrorInner::new_file_not_found(Path::new(&target).to_path_buf(), e)
        } else {
            ErrorInner::new_generic_io(e)
        }
    })?;
    if !rg.is_match(&content) {
        return Err(Error::custom(format!(
            "pattern '{}' not found in template for action {}",
            common.pattern, ctx.resolved_action_name
        )));
    }

    let rendered = if common.render {
        Cow::Owned(omni_tera::one_off(
            template,
            format!("template for action {}", ctx.resolved_action_name),
            &tera_ctx_with_data,
        )?)
    } else {
        Cow::Borrowed(template)
    };

    let replaced = rg.replace_all(&content, &rendered[..]);

    sys.fs_write_async(&target, replaced.as_ref()).await?;

    Ok(())
}
