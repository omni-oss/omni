use omni_generator_configurations::RunJavaScriptActionConfiguration;

use crate::{GeneratorSys, action_handlers::HandlerContext, error::Error};

pub async fn run_javascript<'a>(
    config: &RunJavaScriptActionConfiguration,
    ctx: &HandlerContext<'a>,
    _sys: &impl GeneratorSys,
) -> Result<(), Error> {
    if config.supports_dry_run || !ctx.dry_run {
        // todo
    }

    Ok(())
}
