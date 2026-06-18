use omni_generator_configurations::RunCommandActionConfiguration;
use omni_messages::GeneratorEventSubscriber;

use crate::{
    GeneratorSys,
    action_handlers::{HandlerContext, run_custom_commons::run_custom_commons},
    error::Error,
};

pub async fn run_command<'a, S: GeneratorEventSubscriber>(
    config: &RunCommandActionConfiguration,
    ctx: &HandlerContext<'a, S>,
    sys: &impl GeneratorSys,
) -> Result<(), Error> {
    run_custom_commons(&config.command, None, &config.common, ctx, sys).await
}
