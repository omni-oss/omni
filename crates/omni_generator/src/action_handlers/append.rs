use omni_generator_configurations::{
    AppendActionConfiguration, InsertInlineContentEntry,
};

use crate::{
    GeneratorSys,
    action_handlers::{HandlerContext, insert_commons::insert_one},
    error::Error,
};

pub async fn append<'a>(
    config: &AppendActionConfiguration,
    ctx: &HandlerContext<'a>,
    sys: &impl GeneratorSys,
) -> Result<(), Error> {
    let mut entries = vec![];

    for entry in &config.entries {
        let content = sys
            .fs_read_to_string_async(&ctx.generator_dir.join(&entry.file))
            .await?
            .to_string();
        entries.push(InsertInlineContentEntry {
            pattern: entry.pattern.clone(),
            content,
        });
    }

    insert_one(&entries, false, &config.common, ctx, sys).await?;

    Ok(())
}
