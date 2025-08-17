#[derive(Debug, Clone, Default)]
pub struct PresetConfig {
    pub workspace_name: String,
    pub projects: ProjectsPreset,
}

#[derive(Debug, Clone, Default)]
pub struct ProjectsPreset {
    pub count: usize,
    pub content_folder_nesting: usize,
    pub content_leaf_folder_count: usize,
    pub content_files_count_per_leaf_folder: usize,
    pub content_files_extension: String,
    pub content_files_content: String,
}
