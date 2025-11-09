use omni_generator_configurations::AddManyActionConfiguration;

use crate::{action_handlers::HandlerContext, error::Error};

pub async fn add_many<'a>(
    config: &AddManyActionConfiguration,
    _ctx: &HandlerContext<'a>,
) -> Result<(), Error> {
    todo!("{:?}", config)
}
