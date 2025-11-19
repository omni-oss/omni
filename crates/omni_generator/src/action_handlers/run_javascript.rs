use omni_generator_configurations::RunJavaScriptActionConfiguration;

use crate::{GeneratorSys, action_handlers::HandlerContext, error::Error};

pub async fn run_javascript<'a>(
    _config: &RunJavaScriptActionConfiguration,
    _ctx: &HandlerContext<'a>,
    _sys: &impl GeneratorSys,
) -> Result<(), Error> {
    Ok(())
}
