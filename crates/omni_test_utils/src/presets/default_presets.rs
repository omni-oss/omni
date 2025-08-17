use std::sync::LazyLock;

use crate::presets::{PresetConfig, ProjectsPreset};

pub static JS_LARGE: LazyLock<PresetConfig> = LazyLock::new(|| PresetConfig {
    workspace_name: "js_large".to_string(),
    projects: ProjectsPreset {
        count: 1000,
        content_folder_nesting: 5,
        content_leaf_folder_count: 10,
        content_files_count_per_leaf_folder: 10,
        content_files_extension: "js".to_string(),
        content_files_content: "console.log('Hello World!');".to_string(),
    },
});

pub static JS_MEDIUM: LazyLock<PresetConfig> = LazyLock::new(|| PresetConfig {
    workspace_name: "js_medium".to_string(),
    projects: ProjectsPreset {
        count: 500,
        content_folder_nesting: 2,
        content_leaf_folder_count: 5,
        content_files_count_per_leaf_folder: 10,
        content_files_extension: "js".to_string(),
        content_files_content: "console.log('Hello World!');".to_string(),
    },
});

pub static JS_SMALL: LazyLock<PresetConfig> = LazyLock::new(|| PresetConfig {
    workspace_name: "js_small".to_string(),
    projects: ProjectsPreset {
        count: 100,
        content_folder_nesting: 0,
        content_leaf_folder_count: 1,
        content_files_count_per_leaf_folder: 10,
        content_files_extension: "js".to_string(),
        content_files_content: "console.log('Hello World!');".to_string(),
    },
});
