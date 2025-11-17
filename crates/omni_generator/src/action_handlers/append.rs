use omni_generator_configurations::AppendActionConfiguration;

use crate::{GeneratorSys, action_handlers::HandlerContext, error::Error};

pub async fn append<'a>(
    _config: &AppendActionConfiguration,
    _ctx: &HandlerContext<'a>,
    _sys: &impl GeneratorSys,
) -> Result<(), Error> {
    Ok(())
}
