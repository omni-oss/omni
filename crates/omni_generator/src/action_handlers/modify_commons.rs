use omni_generator_configurations::CommonModifyConfiguration;

use crate::{
    GeneratorSys,
    action_handlers::{HandlerContext, utils::get_target_file},
    error::Error,
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
    let content = sys.fs_read_to_string_async(target.as_ref()).await?;
    if !rg.is_match(&content) {
        return Err(Error::custom(format!(
            "pattern '{}' not found in template for action {}",
            common.pattern, ctx.resolved_action_name
        )));
    }

    let rendered = omni_tera::one_off(
        &template,
        format!("template for action {}", ctx.resolved_action_name),
        ctx.tera_context_values,
    )?;

    let replaced = rg.replace_all(&content, &rendered);

    sys.fs_write_async(target.as_ref(), replaced.as_ref())
        .await?;

    Ok(())
}
