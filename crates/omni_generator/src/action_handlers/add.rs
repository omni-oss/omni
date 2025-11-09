use omni_generator_configurations::AddActionConfiguration;

use crate::{action_handlers::HandlerContext, error::Error};

pub async fn add<'a>(
    config: &AddActionConfiguration,
    _ctx: &HandlerContext<'a>,
) -> Result<(), Error> {
    todo!("{:?}", config)
}
