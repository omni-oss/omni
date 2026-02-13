use omni_generator_configurations::PrependActionConfiguration;

use crate::{
    GeneratorSys,
    action_handlers::{HandlerContext, insert_commons::insert_one},
    error::Error,
};

pub async fn prepend<'a>(
    config: &PrependActionConfiguration,
    ctx: &HandlerContext<'a>,
    sys: &impl GeneratorSys,
) -> Result<(), Error> {
    let template = sys
        .fs_read_to_string_async(&ctx.generator_dir.join(&config.file))
        .await?;

    insert_one(&template, true, &config.common, ctx, sys).await?;

    Ok(())
}
