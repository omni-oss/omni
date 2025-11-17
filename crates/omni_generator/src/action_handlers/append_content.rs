use omni_generator_configurations::AppendContentActionConfiguration;

use crate::{GeneratorSys, action_handlers::HandlerContext, error::Error};

pub async fn append_content<'a>(
    _config: &AppendContentActionConfiguration,
    _ctx: &HandlerContext<'a>,
    _sys: &impl GeneratorSys,
) -> Result<(), Error> {
    Ok(())
}
