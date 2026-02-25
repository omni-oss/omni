use std::{borrow::Cow, path::Path};

use omni_generator_configurations::{
    CommonModifyConfiguration, ModifyInlineContentEntry,
};

use crate::{
    GeneratorSys,
    action_handlers::{
        HandlerContext,
        utils::{augment_tera_context, get_target_file},
    },
    error::{Error, ErrorInner},
};

pub async fn modify_one<'a>(
    entries: &'a [ModifyInlineContentEntry],
    common: &'a CommonModifyConfiguration,
    ctx: &HandlerContext<'a>,
    sys: &'a impl GeneratorSys,
) -> Result<(), Error> {
    let target_name = &common.target;
    let target = get_target_file(target_name, ctx, sys).await?;

    let tera_ctx_with_data =
        augment_tera_context(ctx.tera_context_values, Some(&common.data))?;

    let target = omni_tera::one_off(
        &target.to_string_lossy(),
        "output_path",
        &tera_ctx_with_data,
    )?;

    let mut content = sys
        .fs_read_to_string_async(&target)
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ErrorInner::new_file_not_found(
                    Path::new(&target).to_path_buf(),
                    e,
                )
            } else {
                ErrorInner::new_generic_io(e)
            }
        })?
        .into_owned();

    for entry in entries {
        let rg = regex::Regex::new(&entry.pattern)?;
        if !rg.is_match(&content) {
            return Err(Error::custom(format!(
                "pattern '{}' not found in template for action {}",
                &entry.pattern, ctx.resolved_action_name
            )));
        }

        let rendered = if common.render {
            Cow::Owned(omni_tera::one_off(
                &entry.content,
                format!("template for action {}", ctx.resolved_action_name),
                &tera_ctx_with_data,
            )?)
        } else {
            Cow::Borrowed(&entry.content)
        };

        content = rg.replace_all(&content, &rendered[..]).into_owned();
    }

    sys.fs_write_async(&target, &content).await?;

    Ok(())
}
