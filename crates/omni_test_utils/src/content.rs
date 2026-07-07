//! Host-side generation of each project's `src/**` input tree.
//!
//! These files exist so the cache has real inputs to hash; their layout is
//! deterministic for a given [`crate::ContentConfig`], preserving run-to-run
//! reproducibility. This is a host concern (it writes to disk); the pure core
//! only describes the workspace, it does not lay files down.

use std::{fs, path::Path};

/// Write the deterministic `src/**` content tree into `dir`.
pub(crate) fn write_content_tree(
    dir: impl AsRef<Path>,
    folder_nesting: usize,
    leaf_folder_count: usize,
    file_count_per_leaf_folder: usize,
    file_extension: impl AsRef<str>,
    file_content: impl AsRef<str>,
) -> eyre::Result<()> {
    let dir = dir.as_ref();
    let file_extension = file_extension.as_ref();
    let file_content = file_content.as_ref();

    let mut leaf_dirs = vec![];

    for i in 0..leaf_folder_count {
        if folder_nesting == 0 {
            leaf_dirs.push(dir.join(format!("leaf_{}", i)));
        } else {
            let nested_paths = (0..folder_nesting)
                .map(|j| {
                    if j == 0 {
                        format!("root_{}", i)
                    } else {
                        format!("nested_level_{}", j)
                    }
                })
                .collect::<Vec<_>>()
                .join("/");

            leaf_dirs.push(dir.join(nested_paths).join(format!("leaf_{}", i)));
        }
    }

    for l in &leaf_dirs {
        fs::create_dir_all(l)?;

        for i in 0..file_count_per_leaf_folder {
            let file = if file_extension.is_empty() {
                l.join(format!("file_{}.txt", i))
            } else {
                l.join(format!("file_{}.{}", i, file_extension))
            };

            if file_content.contains("%i%") {
                let content = file_content.replace("%i%", &i.to_string());
                fs::write(file, content)?;
            } else {
                fs::write(file, file_content)?;
            }
        }
    }

    Ok(())
}
