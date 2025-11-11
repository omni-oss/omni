use super::add_commons::add_one;
use omni_generator_configurations::AddActionConfiguration;

use crate::{
    GeneratorSys,
    action_handlers::HandlerContext,
    error::{Error, ErrorInner},
};

pub async fn add<'a>(
    config: &AddActionConfiguration,
    ctx: &HandlerContext<'a>,
    sys: &impl GeneratorSys,
) -> Result<(), Error> {
    let template_file = ctx.generator_dir.join(&config.template_file);
    let file = sys
        .fs_read_async(&template_file)
        .await
        .map_err(|e| ErrorInner::new_failed_to_read_file(&template_file, e))?;
    let template_string = String::from_utf8(file.to_vec())?;

    add_one(
        &config.template_file,
        config.base_path.as_deref(),
        |ctx| tera::Tera::one_off(&template_string, ctx, false),
        &config.base.common,
        ctx,
        sys,
    )
    .await?;
    Ok(())
}
