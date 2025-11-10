use std::path::Path;

use omni_discovery::Discovery;
use omni_generator_configurations::AddManyActionConfiguration;
use tera::Tera;

use crate::{
    GeneratorSys,
    action_handlers::{
        HandlerContext,
        utils::{ensure_dir_exists, get_output_path, overwrite},
    },
    error::{Error, ErrorInner},
};

pub async fn add_many<'a>(
    config: &AddManyActionConfiguration,
    ctx: &HandlerContext<'a>,
    sys: &impl GeneratorSys,
) -> Result<(), Error> {
    let glob_patterns = config
        .template_files
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    let ignore_files = [".omniignore".to_string()];
    let discovery = Discovery::new(
        ctx.generator_dir,
        glob_patterns.as_slice(),
        ignore_files.as_slice(),
    );

    let templates = discovery.discover().await?;

    let generator_dir = format!("{}/**", ctx.generator_dir.display());

    let tera = Tera::new(&generator_dir)?;

    for template_file in templates.iter() {
        let stripped_path = template_file
            .strip_prefix(&ctx.generator_dir)
            .expect("should have value");

        let output_path = get_output_path(
            config.base.common.target.as_deref(),
            stripped_path,
            config.base_path.as_deref(),
            ctx,
            &["tpl"],
            sys,
        )
        .await?;
        let expanded_output = tera::Tera::one_off(
            &output_path.to_string_lossy(),
            ctx.tera_context_values,
            false,
        )?;

        let output_path = Path::new(&expanded_output);
        if let Some(did_overwrite) = overwrite(
            &output_path,
            ctx.overwrite.or(config.base.common.overwrite),
            sys,
        )
        .await?
            && !did_overwrite
        {
            trace::info!("Skipped writing to path {}", output_path.display());
            return Ok(());
        }
        ensure_dir_exists(
            &output_path.parent().expect("should have parent"),
            sys,
        )
        .await?;
        let result = tera.render(
            &stripped_path.to_string_lossy(),
            ctx.tera_context_values,
        )?;
        sys.fs_write_async(&output_path, &result)
            .await
            .map_err(|e| {
                ErrorInner::new_failed_to_write_file(&output_path, e)
            })?;
        trace::info!("Wrote to path {}", output_path.display());
    }
    Ok(())
}
