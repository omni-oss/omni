use omni_generator_configurations::AddManyActionConfiguration;

use crate::{GeneratorSys, action_handlers::HandlerContext, error::Error};

pub async fn add_many<'a>(
    config: &AddManyActionConfiguration,
    _ctx: &HandlerContext<'a>,
    _sys: &impl GeneratorSys,
) -> Result<(), Error> {
    todo!("{:?}", config)
}
