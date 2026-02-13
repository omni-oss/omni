use std::borrow::Cow;

use omni_generator_configurations::CommonInsertConfiguration;

use crate::{
    GeneratorSys,
    action_handlers::{
        HandlerContext,
        utils::{augment_tera_context, get_target_file},
    },
    error::Error,
};

pub async fn insert_one<'a>(
    template: &'a str,
    prepend: bool,
    common: &'a CommonInsertConfiguration,
    ctx: &HandlerContext<'a>,
    sys: &'a impl GeneratorSys,
) -> Result<(), Error> {
    let rg = regex::Regex::new(&common.common.pattern)?;
    let target_name = &common.common.target;
    let target = get_target_file(target_name, ctx, sys).await?;

    let content = sys.fs_read_to_string_async(target.as_ref()).await?;
    if !rg.is_match(&content) {
        return Err(Error::custom(format!(
            "pattern '{}' not found in template for action {}",
            common.common.pattern, ctx.resolved_action_name
        )));
    }

    let tera_ctx_with_data = augment_tera_context(
        ctx.tera_context_values,
        Some(&common.common.data),
    )?;

    let rendered = if common.render {
        Cow::Owned(omni_tera::one_off(
            &template,
            format!("template for action {}", ctx.resolved_action_name),
            &tera_ctx_with_data,
        )?)
    } else {
        Cow::Borrowed(template)
    };

    let mut file = vec![];
    let mut stop_inserting = false;
    for line in content.split(&common.separator) {
        let matching = rg.is_match(line);
        let str = &rendered[..];
        if !stop_inserting && prepend && matching {
            file.push(str);
            if common.unique {
                stop_inserting = true;
            }
        }

        file.push(line);

        if !stop_inserting && !prepend && matching {
            file.push(str);
            stop_inserting = true;
            if common.unique {
                stop_inserting = true;
            }
        }
    }

    let file = file.join(&common.separator);

    sys.fs_write_async(target.as_ref(), file.as_str()).await?;

    Ok(())
}
