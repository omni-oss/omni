use std::{borrow::Cow, path::Path};

use maps::{Map, UnorderedMap};
use omni_capabilities::CapabilityRules;
use omni_generator_configurations::{
    CapabilitiesStrictness, Generator, GeneratorConfiguration, OmniPath,
    OverwriteConfiguration,
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
    /// The workspace-level capability floor (subsystem-agnostic, upcast to the
    /// generator profile). The outermost level of the inherited ceiling and the
    /// value forwarded as the workspace floor when a nested `run-generator`
    /// builds its own [`RunConfig`](crate::RunConfig).
    pub workspace_capabilities: &'a CapabilityRules<Generator>,
    /// The inherited capability ceiling: the ordered policy levels of every
    /// ancestor (outermost first — the workspace floor, then each enclosing
    /// generator's own policy). Under the shrink-only model these cap what this
    /// generator and its scripts may do; this generator can only narrow them,
    /// never widen. A nested `run-generator` extends this with the current
    /// generator's own policy before recursing.
    pub inherited_capabilities: &'a [CapabilityRules<Generator>],
    /// The current generator's own capability policy. Applied as the next level
    /// after the inherited ceiling when confining the JS/TS scripts this
    /// generator runs.
    pub capabilities: &'a CapabilityRules<Generator>,
    /// Effective floor-gap strictness for this generator: the most-severe of the
    /// workspace, all ancestor generators, and this generator's own stance.
    /// `run-javascript` combines the action's own stance on top of it.
    pub capabilities_strictness: CapabilitiesStrictness,
    pub env: &'a Map<String, String>,
    pub gen_session: &'a GenSession,
    pub use_input_defaults: bool,
    pub js_script_runner: &'a dyn JsScriptRunner,
    pub input_provider: &'a dyn omni_input_provider::InputProvider<
        omni_generator_configurations::Generator,
    >,
    pub subscriber: &'a S,
    /// Current `run-generator` nesting depth of this generator.
    pub depth: usize,
    /// Maximum allowed nesting depth, propagated to nested runs.
    pub max_depth: usize,
}
