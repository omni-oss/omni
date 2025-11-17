use std::path::Path;

use maps::UnorderedMap;
use omni_generator_configurations::{
    GeneratorConfiguration, OmniPath, OverwriteConfiguration,
};
use value_bag::OwnedValueBag;

#[derive(Debug, Clone, Copy)]
#[allow(unused)]
pub struct HandlerContext<'a> {
    pub dry_run: bool,
    pub output_dir: &'a Path,
    pub generator_dir: &'a Path,
    pub context_values: &'a UnorderedMap<String, OwnedValueBag>,
    pub tera_context_values: &'a tera::Context,
    pub generator_targets: &'a UnorderedMap<String, OmniPath>,
    pub target_overrides: &'a UnorderedMap<String, OmniPath>,
    pub overwrite: Option<OverwriteConfiguration>,
    pub available_generators: &'a [GeneratorConfiguration],
    pub workspace_dir: &'a Path,
    pub resolved_action_name: &'a str,
}
