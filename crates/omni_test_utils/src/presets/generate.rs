use std::path::Path;

use crate::{WorkspaceGenerator, presets::PresetConfig};

pub fn generate(
    dir: impl AsRef<Path>,
    config: &PresetConfig,
) -> eyre::Result<()> {
    let mut b = WorkspaceGenerator::builder();

    b.name(config.workspace_name.clone());

    for i in 0..config.projects.count {
        b.project(|p| {
            p.name(format!("project_{}", i));
            p.cache(|c| {
                c.enabled(true).key(|k| {
                    k.defaults(true);
                    k.files(vec!["./src/**/*.*".to_string()]);

                    Ok(())
                })?;

                Ok(())
            })?;
            p.task("echo", |t| {
                t.command(format!("echo \"Hello World from project_{}\"", i));
                t.dependency("^echo");

                Ok(())
            })?;
            p.file_content(config.projects.content_files_content.clone());
            p.file_extension(config.projects.content_files_extension.clone());
            p.folder_nesting(config.projects.content_folder_nesting);
            p.leaf_folder_count(config.projects.content_leaf_folder_count);

            Ok(())
        })?;
    }

    let generator = b.build()?;

    generator.generate(dir)?;

    Ok(())
}
