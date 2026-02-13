use omni_generator_configurations::AppendContentActionConfiguration;

use crate::{
    GeneratorSys,
    action_handlers::{HandlerContext, insert_commons::insert_one},
    error::Error,
};

pub async fn append_content<'a>(
    config: &AppendContentActionConfiguration,
    ctx: &HandlerContext<'a>,
    sys: &impl GeneratorSys,
) -> Result<(), Error> {
    insert_one(&config.content, false, &config.common, ctx, sys).await?;
    Ok(())
}
