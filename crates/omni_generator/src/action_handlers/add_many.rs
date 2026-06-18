use std::path::Path;

use super::add_commons::add_one;
use omni_discovery::Discovery;
use omni_messages::GeneratorEventSubscriber;
use omni_generator_configurations::AddManyActionConfiguration;

use crate::{GeneratorSys, action_handlers::HandlerContext, error::Error};

pub async fn add_many<'a, S: GeneratorEventSubscriber>(
    config: &AddManyActionConfiguration,
    ctx: &HandlerContext<'a, S>,
    sys: &impl GeneratorSys,
) -> Result<(), Error> {
    let glob_patterns = config
        .files
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

    log::trace!("discovered {} template files", templates.len());

    let generator_dir = format!("{}/**", ctx.generator_dir.display());

    log::trace!("running generator in dir {generator_dir}");

    let templates = templates
        .into_iter()
        .map(|p| {
            let name = p
                .strip_prefix(&ctx.generator_dir)
                .expect("should have value")
                .to_string_lossy()
                .to_string();
            (p, Some(name))
        })
        .collect::<Vec<_>>();

    let tera = omni_tera::new_with_files(&templates)?;

    for template_file in templates.iter().filter_map(|(_, sp)| sp.as_deref()) {
        log::trace!("processing template tile {template_file:?}");

        add_one(
            Path::new(template_file),
            &template_file,
            config.base_path.as_deref(),
            |template_name, ctx| tera.render(&template_name, ctx),
            &config.base.common,
            config.flatten,
            ctx,
            sys,
        )
        .await?;
    }
    Ok(())
}
