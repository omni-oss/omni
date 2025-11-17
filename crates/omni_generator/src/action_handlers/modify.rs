use omni_generator_configurations::ModifyActionConfiguration;

use crate::{
    GeneratorSys,
    action_handlers::{HandlerContext, modify_commons::modify_one},
    error::Error,
};

pub async fn modify<'a>(
    config: &ModifyActionConfiguration,
    ctx: &HandlerContext<'a>,
    sys: &impl GeneratorSys,
) -> Result<(), Error> {
    let template = sys
        .fs_read_to_string_async(&ctx.generator_dir.join(&config.template_file))
        .await?;

    modify_one(&template, &config.common, ctx, sys).await?;

    Ok(())
}
