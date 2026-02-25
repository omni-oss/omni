use std::{borrow::Cow, path::Path};

use omni_generator_configurations::{
    CommonInsertConfiguration, InsertInlineContentEntry,
};

use crate::{
    GeneratorSys,
    action_handlers::{
        HandlerContext,
        utils::{augment_tera_context, get_target_file, map_file_io_error},
    },
    error::{Error, ErrorInner},
};

pub async fn insert_one<'a>(
    entries: &[InsertInlineContentEntry],
    prepend: bool,
    common: &'a CommonInsertConfiguration,
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
        .map_err(|e| map_file_io_error(Path::new(&target), e))?
        .into_owned();

    for entry in entries {
        let rg = regex::Regex::new(&entry.pattern)?;
        if !rg.is_match(&content) {
            return Err(Error::custom(format!(
                "pattern '{}' not found in template for action {}",
                &entry.pattern, ctx.resolved_action_name
            )));
        }

        let rendered: Cow<str> = if common.render {
            Cow::Owned(omni_tera::one_off(
                &entry.content,
                format!("template for action {}", ctx.resolved_action_name),
                &tera_ctx_with_data,
            )?)
        } else {
            Cow::Borrowed(&entry.content)
        };

        let mut stop_inserting = false;
        let mut file: Vec<&str> = vec![];
        for line in content.split(&common.separator) {
            let matching = rg.is_match(line);
            if !stop_inserting && prepend && matching {
                file.push(&rendered);
                if common.unique {
                    stop_inserting = true;
                }
            }

            file.push(&line);

            if !stop_inserting && !prepend && matching {
                file.push(&rendered);
                stop_inserting = true;
                if common.unique {
                    stop_inserting = true;
                }
            }
        }
        content = file.join(&common.separator);
    }

    sys.fs_write_async(&target, content).await.map_err(|e| {
        ErrorInner::new_failed_to_write_file(Path::new(&target), e)
    })?;

    Ok(())
}
