use std::path::{Path, PathBuf};

use maps::UnorderedMap;
use omni_generator_configurations::OverwriteConfiguration;
use value_bag::OwnedValueBag;

#[derive(Debug, Clone, Copy)]
#[allow(unused)]
pub struct HandlerContext<'a> {
    pub dry_run: bool,
    pub output_dir: &'a Path,
    pub generator_dir: &'a Path,
    pub context_values: &'a UnorderedMap<String, OwnedValueBag>,
    pub tera_context_values: &'a tera::Context,
    pub generator_targets: &'a UnorderedMap<String, PathBuf>,
    pub project_targets: &'a UnorderedMap<String, PathBuf>,
    pub overwrite: Option<OverwriteConfiguration>,
}
