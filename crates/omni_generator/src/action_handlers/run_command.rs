use omni_generator_configurations::RunCommandActionConfiguration;

use crate::{
    GeneratorSys,
    action_handlers::{HandlerContext, run_custom_commons::run_custom_commons},
    error::Error,
};

pub async fn run_command<'a>(
    config: &RunCommandActionConfiguration,
    ctx: &HandlerContext<'a>,
    sys: &impl GeneratorSys,
) -> Result<(), Error> {
    run_custom_commons(&config.command, &config.common, ctx, sys).await
}
