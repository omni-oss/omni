use omni_generator_configurations::AddContentActionConfiguration;

use crate::{
    GeneratorSys,
    action_handlers::{HandlerContext, add_commons::add_one},
    error::Error,
};

pub async fn add_content<'a>(
    config: &AddContentActionConfiguration,
    ctx: &HandlerContext<'a>,
    sys: &impl GeneratorSys,
) -> Result<(), Error> {
    add_one(
        &config.output_path,
        &config.content,
        None,
        |content, ctx| {
            omni_tera::one_off(
                content,
                config.output_path.to_string_lossy().as_ref(),
                ctx,
            )
        },
        &config.base.common,
        false,
        ctx,
        sys,
    )
    .await?;

    Ok(())
}
