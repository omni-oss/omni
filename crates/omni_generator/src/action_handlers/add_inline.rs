use omni_generator_configurations::AddInlineActionConfiguration;

use crate::{
    GeneratorSys,
    action_handlers::{HandlerContext, add_commons::add_one},
    error::Error,
};

pub async fn add_inline<'a>(
    config: &AddInlineActionConfiguration,
    ctx: &HandlerContext<'a>,
    sys: &impl GeneratorSys,
) -> Result<(), Error> {
    add_one(
        &config.output_path,
        None,
        |ctx| {
            omni_tera::one_off(
                &config.template,
                config.output_path.to_string_lossy().as_ref(),
                ctx,
            )
        },
        &config.base.common,
        ctx,
        sys,
    )
    .await?;

    Ok(())
}
