use omni_generator_configurations::ModifyContentActionConfiguration;

use crate::{
    GeneratorSys,
    action_handlers::{HandlerContext, modify_commons::modify_one},
    error::Error,
};

pub async fn modify_content<'a>(
    config: &ModifyContentActionConfiguration,
    ctx: &HandlerContext<'a>,
    sys: &impl GeneratorSys,
) -> Result<(), Error> {
    modify_one(&config.template, &config.common, ctx, sys).await?;

    Ok(())
}
