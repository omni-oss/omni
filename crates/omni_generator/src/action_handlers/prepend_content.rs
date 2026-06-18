use omni_generator_configurations::PrependContentActionConfiguration;
use omni_messages::GeneratorEventSubscriber;

use crate::{
    GeneratorSys,
    action_handlers::{HandlerContext, insert_commons::insert_one},
    error::Error,
};

pub async fn prepend_content<'a, S: GeneratorEventSubscriber>(
    config: &PrependContentActionConfiguration,
    ctx: &HandlerContext<'a, S>,
    sys: &impl GeneratorSys,
) -> Result<(), Error> {
    insert_one(&config.entries, true, &config.common, ctx, sys).await?;
    Ok(())
}
