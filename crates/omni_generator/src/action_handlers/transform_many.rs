use omni_generator_configurations::TransformManyActionConfiguration;
use omni_messages::GeneratorEventSubscriber;

use crate::{
    GeneratorSysFull,
    action_handlers::{HandlerContext, transform_commons::transform_one},
    error::Error,
};

pub async fn transform_many<'a, S: GeneratorEventSubscriber>(
    config: &TransformManyActionConfiguration,
    ctx: &HandlerContext<'a, S>,
    sys: &impl GeneratorSysFull,
) -> Result<(), Error> {
    let patterns = config
        .files
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

    // Match the patterns against the files written so far in this generation,
    // anchored to the output directory.
    let files = sys
        .fs_glob_async(ctx.output_dir, patterns.as_slice())
        .await?;

    log::trace!("transform-many matched {} file(s)", files.len());

    for file in files {
        transform_one(&file, &config.command, &config.common, ctx, sys).await?;
    }

    Ok(())
}
