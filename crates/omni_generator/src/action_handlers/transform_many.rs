use omni_generator_configurations::TransformManyActionConfiguration;
use omni_glob::GlobMatcher;
use omni_messages::GeneratorEventSubscriber;

use crate::{
    GeneratorSysFull,
    action_handlers::{HandlerContext, transform_commons::transform_one},
    error::Error,
};

#[allow(clippy::result_large_err)]
pub async fn transform_many<'a, S: GeneratorEventSubscriber>(
    config: &TransformManyActionConfiguration,
    ctx: &HandlerContext<'a, S>,
    sys: &impl GeneratorSysFull,
) -> Result<(), Error> {
    let (include, exclude) =
        config.files.clone().normalize().to_pattern_strings();

    // Match the include patterns against the files written so far in this
    // generation, anchored to the output directory.
    let candidates = sys
        .fs_glob_async(ctx.output_dir, include.as_slice())
        .await?;

    // Drop anything an exclude pattern matches. exclude always wins.
    let files = if exclude.is_empty() {
        candidates
    } else {
        let exclude_matcher = GlobMatcher::from_globs_rooted(
            ctx.output_dir,
            exclude.as_slice(),
            &[] as &[&str],
            Default::default(),
        )
        .map_err(|e| Error::custom(format!("invalid glob pattern: {e}")))?;
        candidates
            .into_iter()
            .filter(|f| !exclude_matcher.is_match(f))
            .collect()
    };

    log::trace!("transform-many matched {} file(s)", files.len());

    for file in files {
        transform_one(&file, &config.command, &config.common, ctx, sys).await?;
    }

    Ok(())
}
