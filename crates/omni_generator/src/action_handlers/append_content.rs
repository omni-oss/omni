use omni_generator_configurations::AppendContentActionConfiguration;
use omni_messages::GeneratorEventSubscriber;

use crate::{
    GeneratorSys,
    action_handlers::{HandlerContext, insert_commons::insert_one},
    error::Error,
};

pub async fn append_content<'a, S: GeneratorEventSubscriber>(
    config: &AppendContentActionConfiguration,
    ctx: &HandlerContext<'a, S>,
    sys: &impl GeneratorSys,
) -> Result<(), Error> {
    insert_one(&config.entries, false, &config.common, ctx, sys).await?;
    Ok(())
}
