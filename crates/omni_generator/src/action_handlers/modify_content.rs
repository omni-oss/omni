use omni_generator_configurations::ModifyContentActionConfiguration;
use omni_messages::GeneratorEventSubscriber;

use crate::{
    GeneratorSys,
    action_handlers::{HandlerContext, modify_commons::modify_one},
    error::Error,
};

pub async fn modify_content<'a, S: GeneratorEventSubscriber>(
    config: &ModifyContentActionConfiguration,
    ctx: &HandlerContext<'a, S>,
    sys: &impl GeneratorSys,
) -> Result<(), Error> {
    modify_one(&config.entries, &config.common, ctx, sys).await?;

    Ok(())
}
