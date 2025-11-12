use super::add_commons::add_one;
use omni_discovery::Discovery;
use omni_generator_configurations::AddManyActionConfiguration;

use crate::{GeneratorSys, action_handlers::HandlerContext, error::Error};

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

    let tera = omni_tera::new(&generator_dir)?;

    for template_file in templates.iter() {
        let stripped_path = template_file
            .strip_prefix(&ctx.generator_dir)
            .expect("should have value");

        add_one(
            &template_file,
            config.base_path.as_deref(),
            |ctx| tera.render(&stripped_path.to_string_lossy(), ctx),
            &config.base.common,
            ctx,
            sys,
        )
        .await?;
    }
    Ok(())
}
