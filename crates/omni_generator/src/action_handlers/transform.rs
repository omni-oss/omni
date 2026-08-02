use std::path::{Path, PathBuf};

use omni_generator_configurations::TransformActionConfiguration;
use omni_messages::GeneratorEventSubscriber;

use crate::{
    GeneratorSys,
    action_handlers::{HandlerContext, transform_commons::transform_one},
    error::Error,
};

pub async fn transform<'a, S: GeneratorEventSubscriber>(
    config: &TransformActionConfiguration,
    ctx: &HandlerContext<'a, S>,
    sys: &impl GeneratorSys,
) -> Result<(), Error> {
    let file = resolve_file(&config.file, ctx)?;

    transform_one(&file, &config.command, &config.common, ctx, sys).await
}

/// Resolves the (tera-expanded) `file` to an absolute path, anchoring relative
/// paths to the output directory.
fn resolve_file<S: GeneratorEventSubscriber>(
    file: &Path,
    ctx: &HandlerContext<'_, S>,
) -> Result<PathBuf, Error> {
    let expanded = omni_tera::one_off(
        &file.to_string_lossy(),
        "transform file path",
        ctx.tera_context_values,
    )?;

    let path = Path::new(&expanded);

    Ok(if path.is_absolute() {
        path.to_path_buf()
    } else {
        ctx.output_dir.join(path)
    })
}
