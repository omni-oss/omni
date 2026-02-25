use omni_generator_configurations::{
    ModifyActionConfiguration, ModifyInlineContentEntry,
};

use crate::{
    GeneratorSys,
    action_handlers::{HandlerContext, modify_commons::modify_one},
    error::Error,
};

pub async fn modify<'a>(
    config: &ModifyActionConfiguration,
    ctx: &HandlerContext<'a>,
    sys: &impl GeneratorSys,
) -> Result<(), Error> {
    let mut entries = vec![];

    for entry in &config.entries {
        let content = sys
            .fs_read_to_string_async(&ctx.generator_dir.join(&entry.file))
            .await?
            .to_string();
        entries.push(ModifyInlineContentEntry {
            pattern: entry.pattern.clone(),
            content,
        });
    }

    modify_one(&entries, &config.common, ctx, sys).await?;

    Ok(())
}
