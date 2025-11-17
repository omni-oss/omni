use omni_generator_configurations::PrependContentActionConfiguration;

use crate::{
    GeneratorSys,
    action_handlers::{HandlerContext, insert_commons::insert_one},
    error::Error,
};

pub async fn prepend_content<'a>(
    config: &PrependContentActionConfiguration,
    ctx: &HandlerContext<'a>,
    sys: &impl GeneratorSys,
) -> Result<(), Error> {
    insert_one(&config.template, true, &config.common, ctx, sys).await?;
    Ok(())
}
