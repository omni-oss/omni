//! Presence-aware configuration layers.
//!
//! A layer mirrors a configuration file as written: every field is optional,
//! so an omitted field is distinguishable from one set to its default, and the
//! original `append`/`prepend`/`replace`/`merge` operators are kept for replay.
//! Layers apply no defaults and enforce no required fields (such as the project
//! name); that happens when composed layers are finalized.
//!
//! Merging layers follows the same per-field rules as the eager types, with an
//! omitted field contributing nothing. An explicit `null` deserializes as
//! omitted. `base` is the exception: it is a per-declaration marker, so the
//! later layer's value always replaces the earlier one, as for
//! [`crate::ProjectConfiguration`].

use std::path::Path;
use std::sync::Arc;

use config_utils::{DictConfig, DynValue, ListConfig, Replace};
use enum_map::EnumMap;
use merge::Merge;
use omni_command_config::CommandConfig;
use omni_config_types::{SingleOrMany, TeraExprBoolean};
use omni_core::TaskAliases;
use omni_glob_config::MergeGlobConfig;
use omni_retry_config::RetryConfiguration;
use omni_task_output_logs::OutputLogsConfiguration;
use omni_types::{OmniPath, Root};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use system_traits::FsReadAsync;

use crate::{
    CONFIGURED_TASK_CACHE_ENABLED_FALLBACK, CacheConfiguration, CacheEnabled,
    CacheEnabledSource, CacheKeyConfiguration, CacheScope, DeclarationSource,
    Declared, LayerId, LoadConfigError, MetaConfiguration, Origin,
    ProjectConfiguration, ProjectEnvConfiguration, RedactConfiguration,
    TaskConfiguration, TaskConfigurationLongForm, TaskDependencyConfiguration,
    TaskEnvConfiguration, TaskOutputConfiguration, replace_if_present,
    resolve_plugin_tier, utils,
};

#[derive(
    Debug, Clone, Default, PartialEq, Serialize, Deserialize, Merge, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct TaskOutputLayer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub files: Option<MergeGlobConfig<OmniPath>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = config_utils::replace_if_some)]
    pub logs: Option<bool>,
}

#[derive(
    Debug, Clone, Default, PartialEq, Serialize, Deserialize, Merge, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct CacheKeyLayer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = config_utils::replace_if_some)]
    pub defaults: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub env: Option<ListConfig<Replace<String>>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub files: Option<MergeGlobConfig<OmniPath>>,
}

#[derive(
    Debug, Clone, Default, PartialEq, Serialize, Deserialize, Merge, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct CacheLayer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub key: Option<CacheKeyLayer>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub enabled: Option<Replace<bool>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub remote: Option<Replace<bool>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub output: Option<TaskOutputLayer>,
}

#[derive(
    Debug, Clone, Default, PartialEq, Serialize, Deserialize, Merge, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ProjectEnvLayer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub vars: Option<DictConfig<Replace<String>>>,
}

#[derive(
    Debug, Clone, Default, PartialEq, Serialize, Deserialize, Merge, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct TaskLongFormLayer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = config_utils::replace)]
    pub base: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = config_utils::replace_if_some)]
    pub extends: Option<SingleOrMany<String>>,

    #[serde(
        default,
        alias = "command",
        skip_serializing_if = "Option::is_none"
    )]
    #[merge(strategy = merge::option::recurse)]
    pub exec: Option<Replace<CommandConfig>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub retry_exec: Option<Replace<CommandConfig>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub args: Option<DictConfig<DynValue>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub dependencies: Option<ListConfig<TaskDependencyConfiguration>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub with: Option<ListConfig<TaskDependencyConfiguration>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub description: Option<Replace<String>>,

    #[serde(default, alias = "if", skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub enabled: Option<TeraExprBoolean>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub interactive: Option<Replace<bool>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub persistent: Option<Replace<bool>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub env: Option<TaskEnvConfiguration>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub redact: Option<RedactConfiguration>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub cache: Option<CacheLayer>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub output_logs: Option<OutputLogsConfiguration>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub meta: Option<MetaConfiguration>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub retry: Option<RetryConfiguration>,
}

/// One declaration of a task and where it came from.
#[derive(Debug, Clone, PartialEq)]
struct Declaration {
    task: Arc<TaskLongFormLayer>,
    origin: Arc<Origin>,
}

/// The ordered declarations of a composed task. The graphs can share immutable
/// declarations when they clone an extension closure. A short form that
/// joins a composition carries no origin of its own yet: it declares no
/// tracked field.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskDeclarations(Vec<Declaration>);

impl Merge for TaskDeclarations {
    fn merge(&mut self, other: Self) {
        self.0.extend(other.0);
    }
}

impl TaskDeclarations {
    fn one(task: Box<TaskLongFormLayer>, origin: Arc<Origin>) -> Self {
        Self(vec![Declaration {
            task: Arc::from(task),
            origin,
        }])
    }

    pub(crate) fn base(&self) -> bool {
        self.0.last().and_then(|d| d.task.base) == Some(true)
    }

    pub(crate) fn extends(&self) -> Option<&SingleOrMany<String>> {
        self.0.iter().rev().find_map(|d| d.task.extends.as_ref())
    }

    fn fold(
        self,
        mut visit: impl FnMut(&TaskLongFormLayer, &Arc<Origin>),
    ) -> TaskLongFormLayer {
        let mut task = TaskLongFormLayer::default();
        for Declaration {
            task: declared,
            origin,
        } in self.0
        {
            visit(&declared, &origin);
            task.merge(Arc::unwrap_or_clone(declared));
        }
        task
    }
}

/// A parsed task or the ordered declarations produced by composing it.
#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum TaskLayer {
    ShortForm(CommandConfig),
    LongForm(Box<TaskLongFormLayer>),
    /// Intermediate state only. Raw files cannot declare a sequence of layers.
    #[serde(skip)]
    #[schemars(skip)]
    Composed(TaskDeclarations),
    /// Another name for a canonical task, not a task of its own: `{ alias:
    /// <target> }`. It is never a long form, so it cannot also carry task
    /// settings.
    Alias(TaskAliasLayer),
}

/// The target of a [`TaskLayer::Alias`], carrying no task settings. The target
/// is a same-project reference in the shared reference grammar, decoded when the
/// composed project's alias index is built.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskAliasLayer {
    /// The task this name stands for. Write `\#`, `\:` and `\\` for a literal
    /// `#`, `:` and `\` in the task name.
    pub alias: String,
}

/// A map that first yields an already-read key, then continues with the rest.
struct PeekedMap<'de, A: serde::de::MapAccess<'de>> {
    first: Option<String>,
    rest: A,
    _marker: std::marker::PhantomData<&'de ()>,
}

impl<'de, A: serde::de::MapAccess<'de>> serde::de::MapAccess<'de>
    for PeekedMap<'de, A>
{
    type Error = A::Error;

    fn next_key_seed<K>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Self::Error>
    where
        K: serde::de::DeserializeSeed<'de>,
    {
        use serde::de::IntoDeserializer as _;
        match self.first.take() {
            Some(key) => {
                seed.deserialize(key.into_deserializer()).map(Some).map_err(
                    |e: serde::de::value::Error| serde::de::Error::custom(e),
                )
            }
            None => self.rest.next_key_seed(seed),
        }
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::DeserializeSeed<'de>,
    {
        self.rest.next_value_seed(seed)
    }
}

fn deserialize_task_object<'de, A>(mut map: A) -> Result<TaskLayer, A::Error>
where
    A: serde::de::MapAccess<'de>,
{
    use serde::de::Error as _;
    let Some(first) = map.next_key::<String>()? else {
        return TaskLongFormLayer::deserialize(
            serde::de::value::MapAccessDeserializer::new(map),
        )
        .map(|task| TaskLayer::LongForm(Box::new(task)));
    };
    if first == "alias" {
        let target: String = map.next_value()?;
        if let Some(other) = map.next_key::<String>()? {
            return Err(A::Error::custom(format!(
                "an alias entry holds only `alias`, found `{other}` beside it"
            )));
        }
        return Ok(TaskLayer::Alias(TaskAliasLayer { alias: target }));
    }
    let peeked = PeekedMap {
        first: Some(first),
        rest: map,
        _marker: std::marker::PhantomData,
    };
    TaskLongFormLayer::deserialize(
        serde::de::value::MapAccessDeserializer::new(peeked),
    )
    .map(|task| TaskLayer::LongForm(Box::new(task)))
}

impl<'a> Deserialize<'a> for TaskLayer {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'a>,
    {
        use omni_serde_validators::tera_expr::validate_str;
        serde_untagged::UntaggedEnumVisitor::new()
            .string(|s| {
                validate_str(&s).map_err(serde::de::Error::custom)?;
                Ok(TaskLayer::ShortForm(CommandConfig::Shell(s.to_string())))
            })
            .seq(|seq| {
                let items: Vec<String> = seq.deserialize()?;
                for item in &items {
                    validate_str(item).map_err(serde::de::Error::custom)?;
                }
                Ok(TaskLayer::ShortForm(CommandConfig::Argv(items)))
            })
            .map(|map| deserialize_task_object(map))
            .deserialize(deserializer)
    }
}

/// Nameless merge, required by `DictConfig`. A short form expands to an
/// implicit dependency on its own name, so only long/long pairs can be
/// composed here; use [`TaskLayer::merge_named`] (as [`ProjectLayer`] does) for
/// every other pair.
impl Merge for TaskLayer {
    fn merge(&mut self, other: Self) {
        match (&*self, &other) {
            (
                Self::LongForm(_) | Self::Composed(_),
                Self::LongForm(_) | Self::Composed(_),
            ) => {
                // Neither operand needs a shorthand name.
                self.merge_named(other, "");
            }
            _ => *self = other,
        }
    }
}

impl TaskLayer {
    /// Resolve this task to a long-form layer without applying inherited
    /// project settings. An alias has no long form.
    pub fn into_long_form(self, name: &str) -> Option<TaskLongFormLayer> {
        self.into_declarations(name)
            .map(|declarations| declarations.fold(|_, _| {}))
    }

    /// Compose a higher layer's declaration of task `name` over this one.
    ///
    /// Two normal tasks merge field by field whatever their written form; a
    /// shorthand contributes only its command and implicit upstream
    /// dependency. A change between a normal task and an alias, or between two
    /// aliases, replaces the entry as a whole.
    pub fn merge_named(&mut self, other: Self, name: &str) {
        match (&mut *self, other) {
            (TaskLayer::ShortForm(_), other @ TaskLayer::ShortForm(_)) => {
                *self = other;
            }
            (_, other @ TaskLayer::Alias(_)) | (TaskLayer::Alias(_), other) => {
                *self = other
            }
            (this, other) => {
                let right = other
                    .into_declarations(name)
                    .expect("alias pairs are handled above");
                if let TaskLayer::Composed(left) = this {
                    left.merge(right);
                } else {
                    let mut left = this
                        .clone()
                        .into_declarations(name)
                        .expect("alias pairs are handled above");
                    left.merge(right);
                    *this = TaskLayer::Composed(left);
                }
            }
        }
    }
}

/// Merge two `tasks` maps with the semantics of `DictConfig::merge` (the later
/// layer's operator wins; `replace` drops the earlier map), but composing each
/// task through [`TaskLayer::merge_named`].
fn merge_tasks(
    base: &mut Option<DictConfig<TaskLayer>>,
    other: Option<DictConfig<TaskLayer>>,
) {
    let Some(other) = other else { return };
    let Some(current) = base.take() else {
        *base = Some(other);
        return;
    };
    *base = Some(match other {
        DictConfig::Replace { replace } => DictConfig::Replace { replace },
        DictConfig::Value(other_map) => {
            DictConfig::Value(merge_task_maps(current.into_map(), other_map))
        }
        DictConfig::Merge { merge } => DictConfig::Merge {
            merge: merge_task_maps(current.into_map(), merge),
        },
    });
}

fn merge_task_maps(
    mut base: maps::Map<String, TaskLayer>,
    other: maps::Map<String, TaskLayer>,
) -> maps::Map<String, TaskLayer> {
    for (name, task) in other {
        match base.get_mut(&name) {
            Some(existing) => existing.merge_named(task, &name),
            None => {
                base.insert(name, task);
            }
        }
    }
    base
}

#[derive(
    Debug, Clone, Default, PartialEq, Serialize, Deserialize, Merge, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ProjectLayer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = config_utils::replace)]
    pub base: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = config_utils::replace_if_some)]
    pub extends: Option<SingleOrMany<OmniPath>>,

    /// Required only once layers are finalized, not when one is parsed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = config_utils::replace_if_some)]
    pub name: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub description: Option<Replace<String>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub dependencies: Option<ListConfig<Replace<String>>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub env: Option<ProjectEnvLayer>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub cache: Option<CacheLayer>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub output_logs: Option<OutputLogsConfiguration>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub meta: Option<MetaConfiguration>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub redact: Option<RedactConfiguration>,

    /// The project's tasks and aliases, keyed by declaration key (the shared
    /// reference grammar: `\#`, `\:`, `\\` and `\^` write literal characters).
    #[serde(
        default,
        deserialize_with = "deserialize_tasks",
        skip_serializing_if = "Option::is_none"
    )]
    #[merge(strategy = merge_tasks)]
    #[schemars(schema_with = "tasks_schema")]
    pub tasks: Option<DictConfig<TaskLayer>>,
}

/// Decode the `tasks` map and normalize its keys from the declaration-key
/// grammar to raw task names, before any layers are merged.
fn deserialize_tasks<'de, D>(
    deserializer: D,
) -> Result<Option<DictConfig<TaskLayer>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error as _;
    let Some(tasks) =
        Option::<DictConfig<TaskLayer>>::deserialize(deserializer)?
    else {
        return Ok(None);
    };
    normalize_task_keys(tasks)
        .map(Some)
        .map_err(D::Error::custom)
}

/// The raw task name a declaration key spells.
pub(crate) fn decode_declaration_key(key: &str) -> Result<String, String> {
    use omni_types::{ReferenceError, ReferenceSurface, TaskReference};
    // Empty and NUL names are rejected with their own diagnostic when the
    // workspace loads, which names the project.
    if key.is_empty() || key.contains('\0') {
        return Ok(key.to_owned());
    }
    let literal = || {
        TaskReference::own(key)
            .map(|reference| reference.format())
            .unwrap_or_else(|_| key.to_owned())
    };
    let reference = TaskReference::parse(key).map_err(|e| {
        format!(
            "invalid task name `{key}`: {e}; write `{}` for a task literally named `{key}`",
            literal()
        )
    })?;
    reference
        .validate_for(ReferenceSurface::DeclarationKey)
        .map_err(|e| {
            format!(
                "invalid task name `{key}`: {e}; write `{}` for a task literally named `{key}`",
                literal()
            )
        })?;
    if reference.plugin().is_some() {
        let e = ReferenceError::NotAllowed {
            surface: ReferenceSurface::DeclarationKey,
            reason: "be plugin-scoped yet; plugin scopes are not supported in this version",
        };
        return Err(format!(
            "invalid task name `{key}`: {e}; write `{}` for a task literally named `{key}`",
            literal()
        ));
    }
    Ok(reference.task().to_owned())
}

fn normalize_task_map(
    map: maps::Map<String, TaskLayer>,
) -> Result<maps::Map<String, TaskLayer>, String> {
    let mut normalized: maps::Map<String, TaskLayer> = maps::map![];
    let mut spelled: maps::Map<String, String> = maps::map![];
    for (key, task) in map {
        let name = decode_declaration_key(&key)?;
        if let Some(first) = spelled.get(&name) {
            return Err(format!(
                "task keys `{first}` and `{key}` both name the task `{name}`"
            ));
        }
        spelled.insert(name.clone(), key);
        normalized.insert(name, task);
    }
    Ok(normalized)
}

fn normalize_task_keys(
    tasks: DictConfig<TaskLayer>,
) -> Result<DictConfig<TaskLayer>, String> {
    Ok(match tasks {
        DictConfig::Value(map) => DictConfig::Value(normalize_task_map(map)?),
        DictConfig::Merge { merge } => DictConfig::Merge {
            merge: normalize_task_map(merge)?,
        },
        DictConfig::Replace { replace } => DictConfig::Replace {
            replace: normalize_task_map(replace)?,
        },
    })
}

/// The authored `tasks` shape, which follows the loader: an operator is the
/// sole key of its object and an object-valued operator wins, so a task named
/// `merge` or `replace` inside no envelope can only be a short form.
fn tasks_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    let task = generator.subschema_for::<TaskLayer>();
    let short = generator.subschema_for::<CommandConfig>();
    schemars::json_schema!({
        "description": "Tasks and aliases by name. `merge` or `replace` as the only key of this object is an operator; declare a task or alias with that name inside an explicit outer envelope.",
        "anyOf": [
            {
                "type": "object",
                "properties": {
                    "merge": { "type": "object", "additionalProperties": task }
                },
                "required": ["merge"],
                "additionalProperties": false
            },
            {
                "type": "object",
                "properties": {
                    "replace": { "type": "object", "additionalProperties": task }
                },
                "required": ["replace"],
                "additionalProperties": false
            },
            {
                "type": "object",
                "properties": { "merge": short, "replace": short },
                "additionalProperties": task
            }
        ]
    })
}

impl CacheLayer {
    /// Resolve `@project` glob patterns against a layer's own directory.
    fn resolve_project_paths(&mut self, bases: &EnumMap<Root, &Path>) {
        let globs = self
            .key
            .iter_mut()
            .filter_map(|key| key.files.as_mut())
            .chain(self.output.iter_mut().filter_map(|o| o.files.as_mut()));
        for glob in globs {
            glob.iter_mut()
                .filter(|path| path.is_rooted(Root::Project))
                .for_each(|path| path.resolve_in_place(bases));
        }
    }
}

impl ProjectLayer {
    /// Parse one configuration file into a layer, without defaults or any
    /// required field.
    #[allow(clippy::result_large_err)]
    pub async fn load<'a>(
        path: impl Into<&'a Path>,
        sys: &(impl FsReadAsync + Send + Sync),
    ) -> Result<Self, LoadConfigError> {
        let path = path.into();
        utils::fs::load_config_async(path, sys).await
    }

    /// Resolve the layer's `@project` cache glob patterns against the
    /// directory of the file it was read from. This is a per-layer origin: it
    /// stays with the layer when it is composed into another project.
    pub fn resolve_origin_paths(&mut self, bases: &EnumMap<Root, &Path>) {
        for cache in self.authored_cache_layers() {
            cache.resolve_project_paths(bases);
        }
    }

    fn authored_cache_layers(
        &mut self,
    ) -> impl Iterator<Item = &mut CacheLayer> {
        self.cache.iter_mut().chain(
            self.tasks
                .iter_mut()
                .flat_map(|tasks| tasks.values_mut())
                .filter_map(|task| match task {
                    TaskLayer::LongForm(task) => task.cache.as_mut(),
                    _ => None,
                }),
        )
    }
}

/// The level of a declared `cache.enabled` value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnabledScope {
    Project,
    /// A canonical task name.
    Task(String),
}

/// A `cache.enabled` declaration that travels beside a layer, so its tier comes
/// from its origin and it never merges into the replayed `cache`. A
/// `DeclarationSource::Contributor` origin is plugin tier; any other origin is
/// authored tier (an applied file).
#[derive(Debug, Clone, PartialEq)]
pub struct EnabledDecl {
    pub scope: EnabledScope,
    pub value: bool,
    pub origin: Arc<Origin>,
}

/// A project layer with the file it was read from and that file's directory.
/// The directory is the layer's path origin; the effective project root of a
/// composed project is the directory of the concrete (last) layer.
///
/// Composing layers replays values and their provenance in one operation, so
/// a tracked value (`cache.enabled`) and its [`Origin`] only change together.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LoadedLayer {
    dir: OmniPath,
    /// The only record of where the layer came from; the file, if any, is
    /// derived from it so the two cannot disagree.
    source: DeclarationSource,
    layer: ProjectLayer,
    /// The winning project-level `cache.enabled` and who declared it.
    /// Maintained only by composition.
    pub(crate) cache_enabled: Option<Declared<bool>>,
    /// Side `cache.enabled` declarations in composition order. Never part of
    /// `layer`; empty for explicit-only composition.
    pub(crate) enabled: Vec<EnabledDecl>,
}

impl LoadedLayer {
    /// A layer read from `file`.
    pub fn from_file(
        file: OmniPath,
        dir: OmniPath,
        layer: ProjectLayer,
    ) -> Self {
        Self {
            source: DeclarationSource::file(file),
            dir,
            layer,
            cache_enabled: None,
            enabled: Vec::new(),
        }
    }

    /// A layer with no file, declared by the fileless source `id`.
    pub fn fileless(id: LayerId, dir: OmniPath, layer: ProjectLayer) -> Self {
        Self {
            source: DeclarationSource::Fileless { id },
            dir,
            layer,
            cache_enabled: None,
            enabled: Vec::new(),
        }
    }

    /// A layer nothing can attribute: anonymous source, empty directory.
    pub fn unattributed(layer: ProjectLayer) -> Self {
        Self::fileless(LayerId::default(), OmniPath::default(), layer)
    }

    /// The same declaration source and directory around a different layer.
    ///
    /// Invariant: `cache_enabled` always describes `layer`. The tracked value
    /// is dropped here and re-derived from the new layer on the next
    /// composition, so a previously composed layer cannot leave a stale value.
    pub fn with_layer(mut self, layer: ProjectLayer) -> Self {
        self.layer = layer;
        self.cache_enabled = None;
        self
    }

    /// The same layer with side `cache.enabled` declarations appended.
    #[allow(dead_code)]
    pub(crate) fn with_enabled(
        mut self,
        decls: impl IntoIterator<Item = EnabledDecl>,
    ) -> Self {
        self.enabled.extend(decls);
        self
    }

    /// The layer as written, plus whatever composition merged into it.
    pub fn layer(&self) -> &ProjectLayer {
        &self.layer
    }

    /// Edit the layer in place. The tracked `cache.enabled` is dropped and
    /// re-derived from the edited layer on the next composition, so an edit
    /// cannot leave a stale value behind. An edit takes ownership of the
    /// setting: after merging, a re-derived `cache.enabled` is credited to
    /// this layer's file, not to the base that originally declared it.
    pub fn update_layer<R>(
        &mut self,
        edit: impl FnOnce(&mut ProjectLayer) -> R,
    ) -> R {
        self.cache_enabled = None;
        edit(&mut self.layer)
    }

    /// The file the layer was read from; empty for a fileless layer.
    pub fn file(&self) -> &OmniPath {
        static NONE: std::sync::OnceLock<OmniPath> = std::sync::OnceLock::new();
        match self.source.file_path() {
            Some(path) => path,
            None => NONE.get_or_init(OmniPath::default),
        }
    }

    pub fn dir(&self) -> &OmniPath {
        &self.dir
    }

    pub fn source(&self) -> &DeclarationSource {
        &self.source
    }

    fn origin(&self) -> Arc<Origin> {
        Arc::new(Origin::new(self.source.clone(), self.dir.clone()))
    }

    /// Attribute everything this layer declares itself to its own origin.
    /// Declarations already attributed by an earlier composition are kept.
    fn attribute(&mut self) {
        let origin = self.origin();
        if self.cache_enabled.is_none() {
            replace_if_present(
                &mut self.cache_enabled,
                self.layer
                    .cache
                    .as_ref()
                    .and_then(|cache| cache.enabled.as_ref())
                    .map(|enabled| *enabled.as_ref()),
                &origin,
            );
        }
        if let Some(tasks) = self.layer.tasks.as_mut() {
            for task in tasks.values_mut() {
                if let TaskLayer::LongForm(long_form) = task {
                    *task = TaskLayer::Composed(TaskDeclarations::one(
                        std::mem::take(long_form),
                        Arc::clone(&origin),
                    ));
                }
            }
        }
    }
}

impl Merge for LoadedLayer {
    fn merge(&mut self, mut other: Self) {
        self.attribute();
        other.attribute();
        if other.cache_enabled.is_some() {
            self.cache_enabled = other.cache_enabled;
        }
        self.enabled.extend(other.enabled);
        self.dir = other.dir;
        self.source = other.source;
        self.layer.merge(other.layer);
    }
}

impl omni_core::ExtensionGraphNode for LoadedLayer {
    type Id = OmniPath;

    fn id(&self) -> &Self::Id {
        self.file()
    }

    fn extendee_ids(&self) -> &[Self::Id] {
        match &self.layer.extends {
            Some(SingleOrMany::Single(path)) => std::slice::from_ref(path),
            Some(SingleOrMany::Many(paths)) => paths.as_slice(),
            None => &[],
        }
    }
}

impl LoadedLayer {
    /// Apply defaults and validate this layer alone as an eager configuration.
    pub fn finalize(self) -> Result<ProjectConfiguration, MissingProjectName> {
        self.layer.finalize()
    }
}

/// A composed project: the default-filled configuration plus the cache each
/// task ends up with once project-level settings are inherited.
#[derive(Debug, Clone, PartialEq)]
pub struct FinalizedProject {
    pub config: ProjectConfiguration,
    /// The concrete (last) layer's file; `None` when it has none. Location is
    /// not part of the authored configuration.
    pub file: Option<OmniPath>,
    /// The concrete layer's directory: the project's root directory.
    pub dir: OmniPath,
    /// Effective cache per task name. Empty for `base` projects, which run no
    /// tasks.
    pub task_cache: maps::Map<String, CacheConfiguration>,
    /// Each task's effective `cache.enabled` with the tier that decided it. The
    /// value always equals what `task_cache` resolves; this adds the origin,
    /// which `Replace<bool>` loses once layers are merged.
    pub task_cache_enabled: maps::Map<String, CacheEnabled>,
    /// Files that supplied effective authored `cache.enabled` settings.
    /// Defaults and declarations without a loaded file have no entry.
    pub task_cache_enabled_file: maps::Map<String, Arc<OmniPath>>,
    /// The origin of each task's effective authored `cache.enabled`, whether
    /// or not it is a file. A host fallback has no entry.
    pub task_cache_enabled_origin: maps::Map<String, Arc<Origin>>,
    /// The level (task or project) of each task's `cache.enabled` when the plugin
    /// tier decided it. Authored decisions carry it in their source instead.
    pub task_cache_enabled_scope: maps::Map<String, CacheScope>,
    /// Each task's effective `meta`: the project's, with every declaration of
    /// the task replayed over it. Empty for `base` projects.
    pub task_meta: maps::Map<String, MetaConfiguration>,
    /// The project's aliases, flattened to canonical task names. Empty for
    /// `base` projects: their aliases are only validated once composed into a
    /// concrete project. Aliases are not tasks and never appear in `config`.
    pub aliases: TaskAliases,
}

/// Why a composed project cannot be finalized.
#[derive(Debug, thiserror::Error)]
pub enum FinalizeError {
    #[error(transparent)]
    MissingName(#[from] MissingProjectName),
    #[error(transparent)]
    TaskExtension(#[from] crate::TaskExtensionError),
    #[error(transparent)]
    TaskAlias(#[from] crate::TaskAliasError),
    #[error(
        "internal error: replayed `cache.enabled` provenance disagrees with the merged value for task `{task}`"
    )]
    ProvenanceMismatch { task: String },
}

/// Side `cache.enabled` declarations folded into per-tier slots: the last
/// declaration of a slot, in composition order, wins.
#[derive(Default)]
struct SideSlots {
    plugin_project: Option<Declared<bool>>,
    applied_project: Option<Declared<bool>>,
    plugin_tasks: std::collections::HashMap<String, Declared<bool>>,
    applied_tasks: std::collections::HashMap<String, Declared<bool>>,
}

struct TaskSideSlots<'a> {
    plugin_task: Option<&'a Declared<bool>>,
    plugin_project: Option<&'a Declared<bool>>,
    applied_task: Option<&'a Declared<bool>>,
    applied_project: Option<&'a Declared<bool>>,
}

impl TaskSideSlots<'_> {
    fn is_empty(&self) -> bool {
        self.plugin_task.is_none()
            && self.plugin_project.is_none()
            && self.applied_task.is_none()
            && self.applied_project.is_none()
    }
}

impl SideSlots {
    fn fold(decls: Vec<EnabledDecl>) -> Self {
        let mut slots = Self::default();
        for decl in decls {
            let plugin = decl.origin.source.contributor_id().is_some();
            let declared = Declared {
                value: decl.value,
                origin: decl.origin,
            };
            match (decl.scope, plugin) {
                (EnabledScope::Project, true) => {
                    slots.plugin_project = Some(declared)
                }
                (EnabledScope::Project, false) => {
                    slots.applied_project = Some(declared)
                }
                (EnabledScope::Task(name), true) => {
                    slots.plugin_tasks.insert(name, declared);
                }
                (EnabledScope::Task(name), false) => {
                    slots.applied_tasks.insert(name, declared);
                }
            }
        }
        slots
    }

    fn for_task(&self, name: &str) -> TaskSideSlots<'_> {
        TaskSideSlots {
            plugin_task: self.plugin_tasks.get(name),
            plugin_project: self.plugin_project.as_ref(),
            applied_task: self.applied_tasks.get(name),
            applied_project: self.applied_project.as_ref(),
        }
    }
}

impl LoadedLayer {
    /// Finalize a fully composed layer: resolve task extensions and inherit
    /// project-level cache settings into each task on the presence-aware
    /// layers, and only then apply defaults and validate the name.
    pub fn finalize_composed(
        mut self,
    ) -> Result<FinalizedProject, FinalizeError> {
        self.attribute();
        let file = self.source.file_path().map(|p| (**p).clone());
        let LoadedLayer {
            dir,
            mut layer,
            cache_enabled: project_declared,
            enabled: side_decls,
            ..
        } = self;
        let side_slots = SideSlots::fold(side_decls);
        if layer.name.is_none() {
            return Err(MissingProjectName.into());
        }
        let mut task_cache = maps::Map::default();
        let mut task_cache_enabled = maps::Map::default();
        let mut task_cache_enabled_file = maps::Map::default();
        let mut task_cache_enabled_origin = maps::Map::default();
        let mut task_cache_enabled_scope = maps::Map::default();
        let mut task_meta = maps::Map::default();
        let mut aliases = TaskAliases::default();
        let mut provenance_mismatch: Option<String> = None;
        let tasks = if layer.base == Some(true) {
            layer.tasks.take().map(finalize_tasks).unwrap_or_default()
        } else if let Some(mut tasks) = layer.tasks.take() {
            let project_name = layer.name.clone().unwrap_or_default();
            let declared = crate::task_alias::take_aliases(&mut tasks);
            let task_names = tasks.keys().cloned().collect();
            let canonical = crate::task_alias::resolve_aliases(
                &project_name,
                &declared,
                &task_names,
            )?;
            let resolved_extensions =
                crate::task_extension::resolve_task_layer_extensions(
                    &mut tasks, &canonical,
                )?;
            aliases = crate::task_alias::finish_aliases(
                &project_name,
                canonical,
                |target| tasks.as_map().contains_key(target),
            )?;
            finalize_tasks_with(tasks, |name, task| {
                let mut cache = layer.cache.clone();
                let mut meta = layer.meta.clone();
                // Fold the task's own settings and its effective project
                // settings from the same declarations, in the same order.
                let mut task_declared: Option<Declared<bool>> = None;
                let task = task.finalize_with(
                    resolved_extensions,
                    |patch, origin| {
                        merge::option::recurse(&mut cache, patch.cache.clone());
                        merge::option::recurse(&mut meta, patch.meta.clone());
                        replace_if_present(
                            &mut task_declared,
                            patch
                                .cache
                                .as_ref()
                                .and_then(|cache| cache.enabled.as_ref())
                                .map(|enabled| *enabled.as_ref()),
                            origin,
                        );
                    },
                )?;
                // The value and tier the program uses come from the replayed
                // declarations. The merged value must agree with them.
                let replay_decided = match (&task_declared, &project_declared) {
                    (Some(declared), _) => Some((
                        CacheEnabledSource::TaskExplicit,
                        declared.clone(),
                    )),
                    (None, Some(declared)) => Some((
                        CacheEnabledSource::ProjectExplicit,
                        declared.clone(),
                    )),
                    (None, None) => None,
                };
                let merged = cache
                    .as_ref()
                    .and_then(|cache| cache.enabled.as_ref())
                    .map(|enabled| *enabled.as_ref());
                if replay_decided.as_ref().map(|(_, declared)| declared.value)
                    != merged
                {
                    provenance_mismatch.get_or_insert_with(|| name.to_owned());
                }
                let side = side_slots.for_task(name);
                let decided = if side.is_empty() {
                    replay_decided
                        .map(|(source, declared)| (source, declared, None))
                } else {
                    // Side declarations take part: authored is the replayed
                    // (explicit) value, else the applied one; the plugin tier
                    // resolves task over project before it competes.
                    let task_auth = task_declared
                        .clone()
                        .or_else(|| side.applied_task.cloned());
                    let project_auth = project_declared
                        .clone()
                        .or_else(|| side.applied_project.cloned());
                    let plugin = resolve_plugin_tier(
                        side.plugin_task.cloned(),
                        side.plugin_project.cloned(),
                    );
                    let resolved = CacheEnabled::resolve(
                        [
                            (
                                CacheEnabledSource::TaskExplicit,
                                task_auth.as_ref().map(|d| d.value),
                            ),
                            (
                                CacheEnabledSource::ProjectExplicit,
                                project_auth.as_ref().map(|d| d.value),
                            ),
                            (
                                CacheEnabledSource::Plugin,
                                plugin.as_ref().map(|(_, d)| d.value),
                            ),
                        ],
                        CONFIGURED_TASK_CACHE_ENABLED_FALLBACK,
                    );
                    let winner = match resolved.source {
                        CacheEnabledSource::TaskExplicit => {
                            task_auth.map(|d| (d, None))
                        }
                        CacheEnabledSource::ProjectExplicit => {
                            project_auth.map(|d| (d, None))
                        }
                        CacheEnabledSource::Plugin => {
                            plugin.map(|(scope, d)| (d, Some(scope)))
                        }
                        _ => None,
                    };
                    winner.map(|(declared, scope)| {
                        // The merged `cache` holds replayed values only, so
                        // write the decided value into the effective cache.
                        cache.get_or_insert_with(Default::default).enabled =
                            Some(Replace::new(declared.value));
                        (resolved.source, declared, scope)
                    })
                };
                let enabled = match decided {
                    Some((source, declared, scope)) => {
                        if let Some(file) = declared.origin.source.file_path() {
                            task_cache_enabled_file
                                .insert(name.to_owned(), Arc::clone(file));
                        }
                        if let Some(scope) = scope {
                            task_cache_enabled_scope
                                .insert(name.to_owned(), scope);
                        }
                        task_cache_enabled_origin
                            .insert(name.to_owned(), declared.origin);
                        CacheEnabled {
                            value: declared.value,
                            source,
                        }
                    }
                    None => CacheEnabled {
                        value: CONFIGURED_TASK_CACHE_ENABLED_FALLBACK,
                        source: CacheEnabledSource::HostFallback,
                    },
                };
                task_cache.insert(
                    name.to_owned(),
                    cache.unwrap_or_default().finalize(),
                );
                task_meta.insert(name.to_owned(), meta.unwrap_or_default());
                task_cache_enabled.insert(name.to_owned(), enabled);
                Some(task)
            })
        } else {
            DictConfig::default()
        };
        if let Some(task) = provenance_mismatch {
            return Err(FinalizeError::ProvenanceMismatch { task });
        }
        Ok(FinalizedProject {
            config: layer.finalize_with_tasks(tasks)?,
            file,
            dir,
            task_cache,
            task_cache_enabled,
            task_cache_enabled_file,
            task_cache_enabled_origin,
            task_cache_enabled_scope,
            task_meta,
            aliases,
        })
    }
}

/// A project layer has no `name`, which every finalized project requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("project configuration has no `name`")]
pub struct MissingProjectName;

impl TaskOutputLayer {
    /// Fill every omitted field with its default.
    pub fn finalize(self) -> TaskOutputConfiguration {
        TaskOutputConfiguration {
            files: self.files.unwrap_or_else(utils::merge_glob_config_default),
            logs: self.logs.unwrap_or(true),
        }
    }
}

impl CacheKeyLayer {
    /// Fill every omitted field with its default.
    pub fn finalize(self) -> CacheKeyConfiguration {
        CacheKeyConfiguration {
            defaults: self.defaults,
            env: self.env.unwrap_or_else(utils::list_config_default),
            files: self.files.unwrap_or_else(utils::merge_glob_config_default),
        }
    }
}

impl CacheLayer {
    /// Fill every omitted field with its default.
    pub fn finalize(self) -> CacheConfiguration {
        CacheConfiguration {
            key: self.key.map(CacheKeyLayer::finalize).unwrap_or_default(),
            enabled: self.enabled,
            remote: self.remote,
            output: self
                .output
                .map(TaskOutputLayer::finalize)
                .unwrap_or_default(),
        }
    }
}

impl ProjectEnvLayer {
    pub fn finalize(self) -> ProjectEnvConfiguration {
        ProjectEnvConfiguration {
            vars: self.vars.unwrap_or_default(),
        }
    }
}

impl TaskLongFormLayer {
    /// Fill every omitted field with its default.
    pub fn finalize(self) -> TaskConfigurationLongForm {
        TaskConfigurationLongForm {
            base: self.base.unwrap_or(false),
            extends: self.extends,
            exec: self.exec,
            retry_exec: self.retry_exec,
            args: self.args.unwrap_or_default(),
            dependencies: self
                .dependencies
                .unwrap_or_else(utils::list_config_default),
            with: self.with.unwrap_or_else(utils::list_config_default),
            description: self.description,
            enabled: self.enabled,
            interactive: self.interactive,
            persistent: self.persistent,
            env: self.env.unwrap_or_default(),
            redact: self.redact.unwrap_or_default(),
            cache: self.cache.map(CacheLayer::finalize).unwrap_or_default(),
            output_logs: self.output_logs,
            meta: self.meta.unwrap_or_default(),
            retry: self.retry,
        }
    }
}

fn unattributed() -> Arc<Origin> {
    Arc::new(Origin::unattributed())
}

impl TaskLayer {
    pub(crate) fn into_declarations(
        self,
        name: &str,
    ) -> Option<TaskDeclarations> {
        Some(match self {
            TaskLayer::Alias(_) => return None,
            TaskLayer::Composed(declarations) => declarations,
            TaskLayer::LongForm(long_form) => {
                TaskDeclarations::one(long_form, unattributed())
            }
            TaskLayer::ShortForm(command) => TaskDeclarations::one(
                Box::new(TaskLongFormLayer {
                    exec: Some(Replace::new(command)),
                    dependencies: Some(ListConfig::append(vec![
                        TaskDependencyConfiguration::Upstream {
                            task: name.to_string(),
                        },
                    ])),
                    ..Default::default()
                }),
                unattributed(),
            ),
        })
    }

    pub(crate) fn needs_extension_resolution(&self) -> bool {
        match self {
            Self::LongForm(task) => {
                task.base == Some(true) || task.extends.is_some()
            }
            Self::Composed(task) => task.base() || task.extends().is_some(),
            Self::ShortForm(_) | Self::Alias(_) => false,
        }
    }

    /// Finalize keeping the written form: a short form stays short. An alias
    /// has no eager task form yet and yields `None`.
    pub fn finalize(self) -> Option<TaskConfiguration> {
        self.finalize_with(false, |_, _| {})
    }

    fn finalize_with(
        self,
        resolved_extensions: bool,
        mut visit: impl FnMut(&TaskLongFormLayer, &Arc<Origin>),
    ) -> Option<TaskConfiguration> {
        let mut task = match self {
            TaskLayer::ShortForm(command) => {
                return Some(TaskConfiguration::ShortForm(command));
            }
            TaskLayer::LongForm(long_form) => {
                visit(&long_form, &unattributed());
                *long_form
            }
            TaskLayer::Composed(declarations) => declarations.fold(visit),
            TaskLayer::Alias(_) => return None,
        };
        if resolved_extensions {
            task.base = None;
            task.extends = None;
        }
        Some(TaskConfiguration::long_form(task.finalize()))
    }
}

fn finalize_tasks(
    tasks: DictConfig<TaskLayer>,
) -> DictConfig<TaskConfiguration> {
    finalize_tasks_with(tasks, |_, layer| layer.finalize())
}

fn finalize_tasks_with(
    tasks: DictConfig<TaskLayer>,
    mut task: impl FnMut(&str, TaskLayer) -> Option<TaskConfiguration>,
) -> DictConfig<TaskConfiguration> {
    let mut finalize = |map: maps::Map<String, TaskLayer>| {
        map.into_iter()
            .filter_map(|(k, v)| {
                let task = task(&k, v)?;
                Some((k, task))
            })
            .collect()
    };
    match tasks {
        DictConfig::Value(m) => DictConfig::Value(finalize(m)),
        DictConfig::Merge { merge } => DictConfig::Merge {
            merge: finalize(merge),
        },
        DictConfig::Replace { replace } => DictConfig::Replace {
            replace: finalize(replace),
        },
    }
}

impl ProjectLayer {
    /// Apply defaults and enforce the required `name`, producing the eager
    /// configuration. The project's location is not part of it.
    pub fn finalize(
        mut self,
    ) -> Result<ProjectConfiguration, MissingProjectName> {
        let tasks = self.tasks.take().map(finalize_tasks).unwrap_or_default();
        self.finalize_with_tasks(tasks)
    }

    fn finalize_with_tasks(
        self,
        tasks: DictConfig<TaskConfiguration>,
    ) -> Result<ProjectConfiguration, MissingProjectName> {
        Ok(ProjectConfiguration {
            base: self.base.unwrap_or(false),
            extends: self.extends,
            name: self.name.ok_or(MissingProjectName)?,
            description: self.description,
            dependencies: self
                .dependencies
                .unwrap_or_else(utils::list_config_default),
            env: self.env.map(ProjectEnvLayer::finalize).unwrap_or_default(),
            cache: self.cache.map(CacheLayer::finalize).unwrap_or_default(),
            output_logs: self.output_logs,
            meta: self.meta.unwrap_or_default(),
            redact: self.redact.unwrap_or_default(),
            tasks,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProjectConfiguration, utils::list_config_default};

    fn layer(json: &str) -> ProjectLayer {
        serde_json::from_str(json).unwrap()
    }

    fn eager(json: &str) -> ProjectConfiguration {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn a_corrupted_loaded_layer_is_an_error_not_a_panic() {
        let mut loaded = LoadedLayer::from_file(
            OmniPath::new("p.yaml"),
            OmniPath::default(),
            layer(r#"{"name":"p","cache":{"enabled":true},"tasks":{"t":"x"}}"#),
        );
        loaded.cache_enabled = Some(Declared {
            value: false,
            origin: Arc::new(Origin::new(
                DeclarationSource::file(OmniPath::new("other.yaml")),
                OmniPath::default(),
            )),
        });
        assert!(matches!(
            loaded.finalize_composed(),
            Err(FinalizeError::ProvenanceMismatch { ref task }) if task == "t"
        ));
    }

    #[test]
    fn editing_a_merged_layer_cannot_cause_a_provenance_mismatch() {
        let merged = || {
            let mut base = LoadedLayer::from_file(
                OmniPath::new("base.yaml"),
                OmniPath::default(),
                layer(r#"{"cache":{"enabled":true}}"#),
            );
            base.merge(LoadedLayer::from_file(
                OmniPath::new("p.yaml"),
                OmniPath::default(),
                layer(r#"{"name":"p","tasks":{"t":"x"}}"#),
            ));
            base
        };

        let unchanged = merged();
        let declared = unchanged.cache_enabled.clone().unwrap();
        assert!(declared.value);
        assert_eq!(
            declared.origin.source.file_path(),
            Some(&Arc::new(OmniPath::new("base.yaml")))
        );
        assert!(unchanged.finalize_composed().is_ok());

        let mut edited = merged();
        edited.update_layer(|layer| {
            layer.cache.as_mut().unwrap().enabled = Some(false.into());
        });
        assert!(edited.cache_enabled.is_none());
        let finalized = edited.finalize_composed().unwrap();
        assert_eq!(
            finalized.config.cache.enabled.map(|e| *e.as_ref()),
            Some(false)
        );
    }

    #[test]
    fn with_layer_drops_a_stale_tracked_value() {
        let mut loaded = LoadedLayer::from_file(
            OmniPath::new("p.yaml"),
            OmniPath::default(),
            layer(r#"{"name":"p","cache":{"enabled":true},"tasks":{"t":"x"}}"#),
        );
        loaded.attribute();
        assert!(loaded.cache_enabled.is_some());
        let loaded = loaded.with_layer(layer(r#"{"name":"p"}"#));
        assert!(loaded.cache_enabled.is_none());
    }

    #[test]
    fn empty_layer_parses_with_every_field_absent() {
        assert_eq!(layer("{}"), ProjectLayer::default());
        assert!(serde_json::from_str::<ProjectConfiguration>("{}").is_err());
    }

    #[test]
    fn null_is_absent() {
        assert_eq!(layer(r#"{"name":null,"cache":null}"#), layer("{}"));
    }

    #[test]
    fn explicit_defaults_are_present() {
        let l = layer(
            r#"{"base":false,"dependencies":[],"cache":{"enabled":true,
            "key":{"defaults":false,"env":[],"files":[]},
            "output":{"files":[],"logs":true}},
            "env":{"vars":{}},"meta":{},"tasks":{}}"#,
        );
        assert_eq!(l.base, Some(false));
        assert_eq!(l.dependencies, Some(ListConfig::value(vec![])));
        let cache = l.cache.unwrap();
        assert_eq!(cache.enabled, Some(Replace::new(true)));
        let key = cache.key.unwrap();
        assert_eq!(key.defaults, Some(false));
        assert_eq!(key.env, Some(ListConfig::value(vec![])));
        assert!(key.files.is_some());
        let out = cache.output.unwrap();
        assert!(out.files.is_some());
        assert_eq!(out.logs, Some(true));
        assert!(l.env.unwrap().vars.is_some());
        assert!(l.meta.is_some());
        assert!(l.tasks.is_some());
    }

    #[test]
    fn omitted_nested_fields_stay_absent() {
        let l = layer(r#"{"cache":{"key":{"env":["A"]}}}"#);
        let cache = l.cache.unwrap();
        assert!(cache.enabled.is_none() && cache.output.is_none());
        let key = cache.key.unwrap();
        assert!(key.defaults.is_none() && key.files.is_none());
    }

    #[test]
    fn glob_sides_keep_their_presence() {
        let l = layer(r#"{"cache":{"key":{"files":{"exclude":["x"]}}}}"#);
        let files = l.cache.unwrap().key.unwrap().files.unwrap();
        assert!(matches!(
            files,
            MergeGlobConfig::IncludeAndExclude {
                include: None,
                exclude: Some(_)
            }
        ));
    }

    #[test]
    fn list_and_dict_operators_are_kept() {
        let l = layer(
            r#"{"dependencies":{"prepend":["a"]},
            "env":{"vars":{"replace":{"A":"1"}}},
            "cache":{"key":{"env":{"replace":["X"]}}}}"#,
        );
        assert!(l.dependencies.unwrap().is_prepend());
        assert!(l.env.unwrap().vars.unwrap().is_replace());
        assert!(l.cache.unwrap().key.unwrap().env.unwrap().is_replace());
    }

    #[test]
    fn mixed_operator_envelope_is_rejected() {
        assert!(
            serde_json::from_str::<ProjectLayer>(
                r#"{"dependencies":{"append":[],"replace":["x"]}}"#
            )
            .is_err()
        );
    }

    #[test]
    fn unknown_fields_are_rejected() {
        assert!(
            serde_json::from_str::<ProjectLayer>(r#"{"bogus":1}"#).is_err()
        );
        assert!(
            serde_json::from_str::<ProjectLayer>(r#"{"cache":{"bogus":1}}"#)
                .is_err()
        );
    }

    #[test]
    fn task_forms_and_aliases() {
        let l = layer(
            r#"{"tasks":{"a":"echo hi","b":["x","y"],
            "c":{"command":"c","if":"true","cache":{"enabled":false}}}}"#,
        );
        let tasks = l.tasks.unwrap();
        let tasks = tasks.as_map();
        assert_eq!(
            tasks["a"],
            TaskLayer::ShortForm(CommandConfig::Shell("echo hi".into()))
        );
        assert!(matches!(
            tasks["b"],
            TaskLayer::ShortForm(CommandConfig::Argv(_))
        ));
        let TaskLayer::LongForm(c) = &tasks["c"] else {
            panic!("long form")
        };
        assert!(c.exec.is_some() && c.enabled.is_some());
        assert!(c.interactive.is_none() && c.retry.is_none());
    }

    #[test]
    fn absent_field_does_not_override_on_merge() {
        let mut base = layer(
            r#"{"name":"a","cache":{"enabled":false,"key":{"defaults":false}}}"#,
        );
        base.merge(layer(r#"{"cache":{"key":{"env":["E"]}}}"#));
        let cache = base.cache.unwrap();
        assert_eq!(cache.enabled, Some(Replace::new(false)));
        assert_eq!(cache.key.unwrap().defaults, Some(false));
        assert_eq!(base.name.as_deref(), Some("a"));
    }

    #[test]
    fn explicit_default_overrides_on_merge() {
        let mut base = layer(r#"{"cache":{"key":{"defaults":false}}}"#);
        base.merge(layer(r#"{"cache":{"key":{"defaults":true}}}"#));
        assert_eq!(base.cache.unwrap().key.unwrap().defaults, Some(true));
    }

    #[test]
    fn base_is_replaced_not_inherited() {
        let mut l = layer(r#"{"base":true}"#);
        l.merge(layer(r#"{"name":"c"}"#));
        assert_eq!(l.base, None);
    }

    #[test]
    fn task_declarations_stay_out_of_the_config_format() {
        let first = serde_json::json!({"dependencies": {"replace": []}});
        let mut task: TaskLayer =
            serde_json::from_value(first.clone()).unwrap();
        assert_eq!(serde_json::to_value(&task).unwrap(), first);

        let append = serde_json::json!({"append": ["lint"]});
        let other = serde_json::json!({"dependencies": append});
        task.merge_named(serde_json::from_value(other).unwrap(), "build");
        assert!(serde_json::to_value(&task).is_err());
        let finalized = serde_json::to_value(task.finalize().unwrap()).unwrap();
        assert_eq!(finalized["dependencies"], append);
    }

    /// Replaying layers equals merging the eager types, for the fields whose
    /// eager defaults are the identity of the merge.
    #[test]
    fn replay_matches_eager_merge() {
        let docs = [
            r#"{"name":"a","extends":"x.yaml","dependencies":["d1"],
            "env":{"vars":{"A":"1","B":"2"}},
            "cache":{"enabled":true,"key":{"env":["E1"],"files":["f1"]},
                     "output":{"files":["o1"],"logs":false}},
            "tasks":{"t":{"exec":"a","dependencies":["^t"]},"s":"echo"}}"#,
            r#"{"name":"b","dependencies":{"prepend":["d0"]},
            "env":{"vars":{"merge":{"B":"3"}}},
            "cache":{"key":{"env":{"replace":["E2"]},"files":{"exclude":["f2"]}}},
            "tasks":{"t":{"exec":"b","cache":{"enabled":false}}}}"#,
            r#"{"name":"c","dependencies":{"append":["d2"]},
            "env":{"vars":{"replace":{"C":"4"}}},
            "tasks":{"s":{"exec":"z"},"u":"u"}}"#,
        ];
        let mut e = eager(docs[0]);
        let mut l = layer(docs[0]);
        for d in &docs[1..] {
            e.merge(eager(d));
            l.merge(layer(d));
        }
        assert_eq!(l.name.as_ref(), Some(&e.name));
        assert_eq!(l.extends, e.extends);
        assert_eq!(l.dependencies.unwrap(), e.dependencies);
        assert_eq!(l.env.unwrap().vars.unwrap(), e.env.vars);
        let (lc, ec) = (l.cache.unwrap(), e.cache);
        assert_eq!(lc.enabled, ec.enabled);
        let lk = lc.key.unwrap();
        // The eager type folds the operator into its default; the layer keeps
        // it, so compare the resulting values.
        assert_eq!(lk.env.unwrap().into_vec(), ec.key.env.into_vec());
        assert_eq!(lk.files.unwrap().normalize(), ec.key.files.normalize());
        let lo = lc.output.unwrap();
        assert_eq!(lo.files.unwrap().normalize(), ec.output.files.normalize());
        // Known divergence: the eager `logs` is a plain bool, so a later layer
        // that omits it resets it to `true`. The layer keeps the earlier value.
        assert_eq!(lo.logs, Some(false));
        assert!(ec.output.logs);
        let lt = l.tasks.unwrap();
        let mut keys: Vec<_> = lt.keys().cloned().collect();
        let mut ekeys: Vec<_> = e.tasks.keys().cloned().collect();
        keys.sort();
        ekeys.sort();
        assert_eq!(keys, ekeys);
        let TaskLayer::Composed(declarations) = &lt.as_map()["t"] else {
            panic!()
        };
        assert_eq!(declarations.0.len(), 2);
        let t = declarations.clone().fold(|_, _| {}).finalize();
        let crate::TaskConfiguration::LongForm(et) = &e.tasks.as_map()["t"]
        else {
            panic!()
        };
        assert_eq!(t.exec, et.exec);
        assert_eq!(
            t.dependencies.into_vec(),
            et.dependencies.clone().into_vec()
        );
        assert_eq!(t.cache.enabled, et.cache.enabled);
    }

    fn finalized(json: &str) -> ProjectConfiguration {
        layer(json).finalize().unwrap()
    }

    #[test]
    fn finalizing_a_single_layer_equals_eager_deserialization() {
        let docs = [
            r#"{"name":"a"}"#,
            r#"{"name":"a","base":true,"extends":["x.yaml","y.yaml"],
            "description":"d","dependencies":{"prepend":["p"]},
            "env":{"vars":{"A":"1"}},"output_logs":"never",
            "meta":{"k":"v"},
            "cache":{"enabled":false,"key":{"defaults":false,"env":["E"],
                     "files":{"include":["i"]}},
                     "output":{"files":"o","logs":false}},
            "tasks":{"s":"echo hi","a":["x","y"],
              "t":{"exec":"c","dependencies":["^t"],"with":["w"],
                   "args":{"k":1},"enabled":"true","interactive":true,
                   "cache":{"key":{"env":["T"]},"output":{"logs":false}},
                   "env":{"vars":{"B":"2"}}}}}"#,
            r#"{"name":"a","dependencies":[],"env":{},"cache":{},
            "tasks":{"replace":{"t":{}}}}"#,
        ];
        for doc in docs {
            assert_eq!(finalized(doc), eager(doc), "{doc}");
        }
    }

    #[test]
    fn finalize_applies_defaults_for_omissions() {
        let c = finalized(r#"{"name":"a","cache":{"output":{"files":"o"}}}"#);
        assert!(!c.base);
        assert!(c.cache.output.logs);
        assert_eq!(c.dependencies, list_config_default());
    }

    #[test]
    fn finalize_requires_a_name() {
        let err = layer(r#"{"base":true}"#).finalize().unwrap_err();
        assert_eq!(err, MissingProjectName);
    }

    #[test]
    fn loaded_layer_merge_takes_the_later_origin() {
        let mut a = LoadedLayer::from_file(
            OmniPath::new("a/p.yaml"),
            OmniPath::new("a"),
            layer(r#"{"name":"a","base":true}"#),
        );
        a.merge(LoadedLayer::from_file(
            OmniPath::new("b/p.yaml"),
            OmniPath::new("b"),
            layer(r#"{"description":"d"}"#),
        ));
        assert_eq!(a.file(), &OmniPath::new("b/p.yaml"));
        assert_eq!(a.dir(), &OmniPath::new("b"));
        assert_eq!(a.layer.name.as_deref(), Some("a"));
        assert_eq!(a.layer.base, None);
    }

    fn composed(docs: &[&str]) -> FinalizedProject {
        let mut it = docs.iter().map(|d| LoadedLayer::unattributed(layer(d)));
        let mut acc = it.next().unwrap();
        for next in it {
            acc.merge(next);
        }
        acc.finalize_composed().unwrap()
    }

    /// Where the old behavior was correct (nothing resets an omitted value),
    /// composing layers equals merging eager configurations.
    #[test]
    fn composition_equals_the_eager_merge_where_old_behavior_was_correct() {
        let docs = [
            r#"{"name":"a","dependencies":["d1"],"env":{"vars":{"A":"1"}},
            "cache":{"enabled":true,"key":{"env":["E1"],"files":["f1"]},
                     "output":{"files":["o1"],"logs":false}},
            "tasks":{"tpl":{"base":true,"exec":"t","env":{"vars":{"X":"1"}}},
                     "t":{"extends":"tpl","dependencies":["^t"]},"s":"echo"}}"#,
            r#"{"name":"b","dependencies":{"prepend":["d0"]},
            "cache":{"key":{"files":{"exclude":["f2"]}},
                     "output":{"logs":false}},
            "tasks":{"t":{"exec":"b","cache":{"output":{"logs":false}}}}}"#,
            r#"{"name":"c","dependencies":{"replace":["only"]},
            "tasks":{"s":{"exec":"z"},"u":"u"}}"#,
        ];
        let mut e = eager(docs[0]);
        for d in &docs[1..] {
            e.merge(eager(d));
        }
        let got = composed(&docs).config;
        // Operators are folded differently (`Value` vs `Append`), so compare
        // effective values. `logs` is the documented divergence.
        assert_eq!(got.name, e.name);
        assert_eq!(got.dependencies.into_vec(), e.dependencies.into_vec());
        assert_eq!(got.env, e.env);
        assert_eq!(got.cache.enabled, e.cache.enabled);
        assert_eq!(got.cache.key.env.into_vec(), e.cache.key.env.into_vec());
        assert_eq!(
            got.cache.key.files.normalize(),
            e.cache.key.files.normalize()
        );
        assert_eq!(
            got.cache.output.files.normalize(),
            e.cache.output.files.normalize()
        );
        // Tasks (template resolution is covered in `task_extension`): the
        // `tpl` template is dropped and `t` inherits it.
        let mut names = got.tasks.keys().cloned().collect::<Vec<_>>();
        names.sort();
        assert_eq!(names, ["s", "t", "u"]);
        let t = got.tasks.as_map()["t"].clone().into_long_form("t");
        assert_eq!(
            t.exec,
            Some(Replace::new(CommandConfig::Shell("b".into())))
        );
        assert_eq!(
            t.env
                .vars
                .as_ref()
                .unwrap()
                .to_map_inner()
                .get("X")
                .cloned(),
            Some("1".to_string())
        );
        assert_eq!(
            t.dependencies.into_vec(),
            vec![TaskDependencyConfiguration::Upstream { task: "t".into() }]
        );
        assert!(!t.cache.output.logs);
        // Documented correction (RFC 0024 §17.9): a long form over a short form
        // keeps the shorthand's implicit upstream dependency instead of
        // replacing the whole task.
        let s = got.tasks.as_map()["s"].clone().into_long_form("s");
        assert_eq!(
            s.exec,
            Some(Replace::new(CommandConfig::Shell("z".into())))
        );
        assert_eq!(
            s.dependencies.into_vec(),
            vec![TaskDependencyConfiguration::Upstream { task: "s".into() }]
        );
    }

    #[test]
    fn omitted_logs_survive_composition_and_task_inheritance() {
        let out = composed(&[
            r#"{"name":"a","cache":{"output":{"logs":false}},
            "tasks":{"t":{"exec":"x"},"s":"echo"}}"#,
            r#"{"cache":{"key":{"env":["E"]}},
            "tasks":{"t":{"cache":{"key":{"env":["T"]}}}}}"#,
        ]);
        assert!(!out.config.cache.output.logs);
        assert!(!out.task_cache["t"].output.logs);
        assert!(!out.task_cache["s"].output.logs);
    }

    #[test]
    fn project_cache_remote_false_is_inherited_and_a_task_can_override_it() {
        let out = composed(&[
            r#"{"name":"a","cache":{"remote":false},
            "tasks":{"t":{"exec":"x"},"s":"echo",
                     "u":{"exec":"y","cache":{"remote":true}}}}"#,
            r#"{"tasks":{"t":{"cache":{"key":{"env":["T"]}}}}}"#,
        ]);
        assert_eq!(out.task_cache["t"].remote, Some(Replace::new(false)));
        assert_eq!(out.task_cache["s"].remote, Some(Replace::new(false)));
        assert_eq!(out.task_cache["u"].remote, Some(Replace::new(true)));
    }

    #[test]
    fn explicit_task_default_overrides_the_project_value() {
        let out =
            composed(&[r#"{"name":"a","cache":{"output":{"logs":false}},
            "tasks":{"t":{"cache":{"output":{"logs":true}}}}}"#]);
        assert!(out.task_cache["t"].output.logs);
    }

    #[test]
    fn project_to_task_cache_keeps_per_side_glob_presence() {
        let out = composed(&[
            r#"{"name":"a","cache":{"key":{"files":{"include":["i"],
            "exclude":["x"]}}},
            "tasks":{"t":{"cache":{"key":{"files":{"include":{"append":["j"]}}}}}}}"#,
        ]);
        let files = out.task_cache["t"].key.files.clone().normalize();
        assert_eq!(files.exclude.len(), 1);
        assert_eq!(files.include.len(), 2);
    }

    #[test]
    fn extendee_ids_normalize_forms() {
        use omni_core::ExtensionGraphNode;

        let ids = |json: &str| {
            LoadedLayer::unattributed(layer(json))
                .extendee_ids()
                .to_vec()
        };
        assert_eq!(
            ids(r#"{"name":"p","extends":"a.yaml"}"#),
            [OmniPath::new("a.yaml")]
        );
        assert_eq!(
            ids(r#"{"name":"p","extends":["a.yaml","b.yaml"]}"#),
            [OmniPath::new("a.yaml"), OmniPath::new("b.yaml")]
        );
        assert!(ids(r#"{"name":"p"}"#).is_empty());
    }

    #[test]
    fn base_projects_run_no_tasks_and_skip_task_resolution() {
        let out = composed(&[
            r#"{"name":"a","base":true,"tasks":{"t":{"extends":"gone"}}}"#,
        ]);
        assert!(out.task_cache.is_empty());
        assert!(out.config.tasks.as_map().contains_key("t"));
    }

    #[test]
    fn unknown_task_extendee_fails_composition() {
        let l = LoadedLayer::unattributed(layer(
            r#"{"name":"a","tasks":{"t":{"extends":"gone"}}}"#,
        ));
        assert!(matches!(
            l.finalize_composed(),
            Err(FinalizeError::TaskExtension(_))
        ));
    }

    mod side_enabled {
        use super::*;
        use crate::ContributorId;

        fn plugin(id: &str) -> Arc<Origin> {
            Arc::new(Origin::new(
                DeclarationSource::contributor(ContributorId::new(id)),
                OmniPath::default(),
            ))
        }

        fn applied(file: &str) -> Arc<Origin> {
            Arc::new(Origin::new(
                DeclarationSource::file(OmniPath::new(file)),
                OmniPath::default(),
            ))
        }

        fn task_decl(
            task: &str,
            value: bool,
            origin: Arc<Origin>,
        ) -> EnabledDecl {
            EnabledDecl {
                scope: EnabledScope::Task(task.into()),
                value,
                origin,
            }
        }

        fn project_decl(value: bool, origin: Arc<Origin>) -> EnabledDecl {
            EnabledDecl {
                scope: EnabledScope::Project,
                value,
                origin,
            }
        }

        /// The explicit layer is `doc`; `decls` are side declarations.
        fn finalize(doc: &str, decls: Vec<EnabledDecl>) -> FinalizedProject {
            LoadedLayer::unattributed(layer(doc))
                .with_enabled(decls)
                .finalize_composed()
                .unwrap()
        }

        fn decided(p: &FinalizedProject) -> (bool, CacheEnabledSource) {
            let d = p.task_cache_enabled["t"];
            (d.value, d.source)
        }

        fn effective(p: &FinalizedProject) -> Option<bool> {
            p.task_cache["t"].enabled.map(|e| *e.as_ref())
        }

        const BARE: &str = r#"{"name":"a","tasks":{"t":"echo"}}"#;
        const PROJECT_FALSE: &str =
            r#"{"name":"a","cache":{"enabled":false},"tasks":{"t":"echo"}}"#;

        use CacheEnabledSource::*;

        #[test]
        fn c01_row3_plugin_task_false() {
            let p = finalize(BARE, vec![task_decl("t", false, plugin("a"))]);
            assert_eq!(decided(&p), (false, Plugin));
            assert_eq!(effective(&p), Some(false));
            assert_eq!(p.task_cache_enabled_scope["t"], CacheScope::Task);
        }

        #[test]
        fn c01_row4_plugin_task_true() {
            let p = finalize(BARE, vec![task_decl("t", true, plugin("a"))]);
            assert_eq!(decided(&p), (true, Plugin));
            assert_eq!(effective(&p), Some(true));
        }

        #[test]
        fn c01_row5_plugin_project_false() {
            let p = finalize(BARE, vec![project_decl(false, plugin("a"))]);
            assert_eq!(decided(&p), (false, Plugin));
            assert_eq!(p.task_cache_enabled_scope["t"], CacheScope::Project);
        }

        #[test]
        fn c01_row6_plugin_task_beats_plugin_project() {
            let p = finalize(
                BARE,
                vec![
                    task_decl("t", true, plugin("a")),
                    project_decl(false, plugin("a")),
                ],
            );
            assert_eq!(decided(&p), (true, Plugin));
            assert_eq!(p.task_cache_enabled_scope["t"], CacheScope::Task);
        }

        #[test]
        fn c01_row7_authored_project_beats_plugin_task() {
            let p = finalize(
                PROJECT_FALSE,
                vec![
                    task_decl("t", true, plugin("a")),
                    project_decl(true, plugin("a")),
                ],
            );
            assert_eq!(decided(&p), (false, ProjectExplicit));
            assert_eq!(effective(&p), Some(false));
            assert!(p.task_cache_enabled_scope.is_empty());
        }

        #[test]
        fn c01_row8_authored_task_beats_everything() {
            let p = finalize(
                r#"{"name":"a","cache":{"enabled":true},"tasks":{"t":{"exec":"x","cache":{"enabled":false}}}}"#,
                vec![
                    task_decl("t", true, plugin("a")),
                    project_decl(true, plugin("a")),
                ],
            );
            assert_eq!(decided(&p), (false, TaskExplicit));
            assert_eq!(effective(&p), Some(false));
        }

        #[test]
        fn c01_row9_authored_task_true_over_false_project_and_plugins() {
            let p = finalize(
                r#"{"name":"a","cache":{"enabled":false},"tasks":{"t":{"exec":"x","cache":{"enabled":true}}}}"#,
                vec![
                    task_decl("t", false, plugin("a")),
                    project_decl(false, plugin("a")),
                ],
            );
            assert_eq!(decided(&p), (true, TaskExplicit));
        }

        #[test]
        fn an_applied_task_value_beats_an_explicit_project_value() {
            let p = finalize(
                PROJECT_FALSE,
                vec![task_decl("t", true, applied("apply.yaml"))],
            );
            assert_eq!(decided(&p), (true, TaskExplicit));
            // The replayed project value is replaced in the effective cache.
            assert_eq!(effective(&p), Some(true));
            assert!(p.task_cache_enabled_scope.is_empty());
            assert!(p.task_cache_enabled_file.contains_key("t"));
        }

        #[test]
        fn an_explicit_task_value_beats_an_applied_task_value() {
            let p = finalize(
                r#"{"name":"a","tasks":{"t":{"exec":"x","cache":{"enabled":false}}}}"#,
                vec![task_decl("t", true, applied("apply.yaml"))],
            );
            assert_eq!(decided(&p), (false, TaskExplicit));
            assert_eq!(effective(&p), Some(false));
        }

        #[test]
        fn an_explicit_project_value_beats_an_applied_project_value() {
            let p = finalize(
                PROJECT_FALSE,
                vec![project_decl(true, applied("apply.yaml"))],
            );
            assert_eq!(decided(&p), (false, ProjectExplicit));
        }

        #[test]
        fn an_applied_project_value_beats_a_plugin_task_value() {
            let p = finalize(
                BARE,
                vec![
                    task_decl("t", true, plugin("a")),
                    project_decl(false, applied("apply.yaml")),
                ],
            );
            assert_eq!(decided(&p), (false, ProjectExplicit));
        }

        #[test]
        fn the_later_plugin_wins_within_a_tier_and_scope() {
            let forward = finalize(
                BARE,
                vec![
                    project_decl(true, plugin("a")),
                    project_decl(false, plugin("b")),
                ],
            );
            assert_eq!(decided(&forward), (false, Plugin));
            let reversed = finalize(
                BARE,
                vec![
                    project_decl(false, plugin("b")),
                    project_decl(true, plugin("a")),
                ],
            );
            assert_eq!(decided(&reversed), (true, Plugin));
        }

        #[test]
        fn an_earlier_plugin_task_beats_a_later_plugin_project() {
            let p = finalize(
                BARE,
                vec![
                    task_decl("t", true, plugin("a")),
                    project_decl(false, plugin("b")),
                ],
            );
            assert_eq!(decided(&p), (true, Plugin));
        }

        #[test]
        fn a_plugin_decision_has_an_origin_and_no_file() {
            let p = finalize(BARE, vec![task_decl("t", true, plugin("a"))]);
            assert!(!p.task_cache_enabled_file.contains_key("t"));
            let origin = &p.task_cache_enabled_origin["t"];
            assert_eq!(origin.source.contributor_id().unwrap().as_str(), "a");
        }

        #[test]
        fn a_side_declaration_for_another_task_changes_nothing() {
            let p = finalize(
                r#"{"name":"a","tasks":{"t":"echo","u":"echo"}}"#,
                vec![task_decl("u", false, plugin("a"))],
            );
            assert_eq!(decided(&p), (true, HostFallback));
            assert_eq!(effective(&p), None);
            assert_eq!(p.task_cache_enabled["u"].source, Plugin);
        }

        #[test]
        fn side_declarations_are_not_inherited_through_task_extends() {
            let p = finalize(
                r#"{"name":"a","tasks":{"base":{"base":true,"exec":"x"},"t":{"extends":"base","exec":"y"}}}"#,
                vec![task_decl("base", false, plugin("a"))],
            );
            assert_eq!(decided(&p), (true, HostFallback));
        }

        #[test]
        fn without_side_declarations_the_host_fallback_keeps_none() {
            let p = finalize(BARE, vec![]);
            assert_eq!(decided(&p), (true, HostFallback));
            assert_eq!(effective(&p), None);
        }

        #[test]
        fn merging_layers_keeps_side_declarations_in_order() {
            let mut a = LoadedLayer::unattributed(layer(BARE))
                .with_enabled([project_decl(true, plugin("a"))]);
            a.merge(
                LoadedLayer::unattributed(layer("{}"))
                    .with_enabled([project_decl(false, plugin("b"))]),
            );
            let p = a.finalize_composed().unwrap();
            assert_eq!(decided(&p), (false, Plugin));
        }
    }
}
