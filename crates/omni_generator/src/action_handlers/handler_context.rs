use std::{borrow::Cow, path::Path};

use maps::{Map, UnorderedMap};
use omni_generator_configurations::{
    GeneratorConfiguration, OmniPath, OverwriteConfiguration,
};
use omni_messages::{GeneratorEventSubscriber, NoopSubscriber};
use value_bag::OwnedValueBag;

use crate::{JsScriptRunner, gen_session::GenSession};

#[derive(Debug, Clone, Copy)]
#[allow(unused)]
pub struct HandlerContext<'a, S: GeneratorEventSubscriber = NoopSubscriber> {
    pub dry_run: bool,
    pub output_dir: &'a Path,
    pub generator_dir: &'a Path,
    pub generator_name: &'a str,
    pub scope_id: Option<&'a str>,
    pub current_dir: &'a Path,
    pub context_values: &'a UnorderedMap<String, OwnedValueBag>,
    pub tera_context_values: &'a omni_tera::Context,
    pub generator_targets: &'a UnorderedMap<String, OmniPath>,
    pub target_overrides: &'a UnorderedMap<String, OmniPath>,
    pub overwrite: Option<OverwriteConfiguration>,
    pub available_generators: &'a [Cow<'a, GeneratorConfiguration>],
    pub workspace_dir: &'a Path,
    pub resolved_action_name: &'a str,
    pub env: &'a Map<String, String>,
    pub gen_session: &'a GenSession,
    pub use_input_defaults: bool,
    pub js_script_runner: &'a dyn JsScriptRunner,
    pub input_provider: &'a dyn omni_input_provider::InputProvider,
    pub subscriber: &'a S,
}
