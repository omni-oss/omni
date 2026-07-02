use omni_configurations::types::MaybeExpr;
use omni_generator::Action;
use omni_generator_configurations::ActionConfiguration;
use omni_generator_configurations::ForAllInputValuesConfiguration;
use omni_generator_configurations::ForwardInputValuesConfiguration;
use omni_generator_configurations::GenBase;
use omni_generator_configurations::Generator;
use omni_generator_configurations::GeneratorConfiguration;
use omni_generator_configurations::InputValue;
use omni_generator_configurations::ListWidget;
use omni_generator_configurations::OmniPath;
use omni_generator_configurations::OverwriteConfiguration;
use omni_generator_configurations::StringWidget;
use omni_generator_configurations::gen_base;
use omni_input_provider::AllowedValue;
use omni_input_provider::BaseInput;
use omni_input_provider::Input;
use omni_input_provider::InputProvider;
use omni_input_provider::InputSchema;
use omni_input_provider::ValidationConfig;
use omni_input_provider::builder::boolean;
use omni_input_provider::collect_one;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashSet;
use std::fmt::Debug;
use std::path::PathBuf;
use std::sync::Arc;

use maps::UnorderedMap;
use omni_configurations::{GeneratorSourceConfiguration, types::SingleOrMany};
use omni_context::{Context, ContextSys, LoadedContext};
use omni_generator::{GeneratorSys, RunConfig};
use omni_messages::GeneratorEventSubscriber;
use omni_remote_sources::manager::{
    RemoteSourceManager, config::RemoteSourceConfig,
};
use tokio::task::JoinSet;
use value_bag::{OwnedValueBag, ValueBag};

// ── Request / Response types ──────────────────────────────────────────────────

/// Request to run a generator.
#[derive(Debug, JsonSchema)]
pub struct GeneratorRunRequest {
    /// The generator to run. `None` is not accepted by the API handler — the
    /// CLI adapter must prompt the user and provide the name before calling.
    pub name: Option<String>,
    /// Absolute path to the directory where files will be generated.
    pub output_dir: PathBuf,
    /// Resolve `output_dir` from this project's directory (CLI convenience).
    pub project: Option<String>,
    /// Target overrides: `(target_key, path)` pairs.
    #[schemars(with = "UnorderedMap<String, String>")]
    pub target: UnorderedMap<String, OmniPath>,
    /// Simulate generation without writing files.
    pub dry_run: bool,
    /// How to handle existing files.
    pub overwrite: Option<OverwriteConfiguration>,
    /// Persist the session (inputs + targets) to disk after a successful run.
    pub save_session: Option<bool>,
    /// Skip loading an existing session from disk even if one exists.
    pub ignore_session: Option<bool>,
    /// Pre-filled prompt values (key → value bag).
    #[schemars(with = "UnorderedMap<String, serde_json::Value>")]
    pub input_values: UnorderedMap<String, OwnedValueBag>,
    /// Use default values for all prompts that have defaults.
    pub use_defaults: bool,
    /// Supplies interactive prompts. Use `NoopInputProvider` for non-interactive contexts.
    #[schemars(skip)]
    pub input_provider: Arc<dyn InputProvider<Generator>>,
    /// Maximum `run-generator` nesting depth before the run is aborted. `None`
    /// uses [`omni_generator::DEFAULT_MAX_GENERATOR_DEPTH`].
    pub max_depth: Option<usize>,
}

/// Response from a `generator_run` call.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GeneratorRunResponse {
    /// Actions done by the generator (empty for dry runs).
    pub actions: Vec<Action>,
    /// `true` if the session was saved to disk.
    pub session_saved: bool,
}

/// A single entry in a `generator_list` response.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GeneratorInfo {
    pub name: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
}

/// Response from a `generator_list` call.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GeneratorListResponse {
    pub generators: Vec<GeneratorInfo>,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// Run a generator.
///
/// `subscriber` is passed by reference (the `&S` blanket impl covers forwarding).
pub async fn handle_generator_run<TSys, S>(
    ctx: &LoadedContext<TSys>,
    subscriber: &S,
    req: GeneratorRunRequest,
) -> eyre::Result<GeneratorRunResponse>
where
    TSys: ContextSys + GeneratorSys + Clone,
    S: GeneratorEventSubscriber,
{
    let name = req.name.ok_or_else(|| {
        eyre::eyre!(
            "generator name is required; the CLI adapter must prompt the user before calling generator_run"
        )
    })?;

    let sys = ctx.sys().clone();

    let projects = ctx.projects();
    let current_dir = ctx.current_dir()?;
    let workspace_dir = ctx.root_dir().to_path_buf();

    // Resolve output_dir and optionally the project.
    let (output_dir, project) = match (&req.project, req.output_dir) {
        (Some(proj_name), _) => {
            let p = projects.iter().find(|p| p.name == *proj_name);
            match p {
                Some(p) => (p.dir.clone(), Some(p)),
                None => {
                    return Err(eyre::eyre!(
                        "project '{}' not found",
                        proj_name
                    ));
                }
            }
        }
        (None, dir) => (path_clean::clean(current_dir.join(&dir)), None),
    };

    // Build context values map fed into Tera templates.
    let default_map = maps::Map::default();
    let env = ctx.get_cached_env_vars(output_dir.as_path());

    let mut context_values: UnorderedMap<String, OwnedValueBag> =
        Default::default();
    context_values.insert(
        "output_dir".to_string(),
        ValueBag::capture_serde1(&output_dir).to_owned(),
    );
    context_values.insert(
        "workspace_dir".to_string(),
        ValueBag::capture_serde1(&workspace_dir).to_owned(),
    );
    context_values.insert(
        "current_dir".to_string(),
        ValueBag::capture_serde1(&current_dir).to_owned(),
    );
    context_values.insert(
        "env".to_string(),
        ValueBag::capture_serde1(env.as_deref().unwrap_or(&default_map))
            .to_owned(),
    );
    if let Some(project) = project {
        context_values.insert(
            "project".to_string(),
            ValueBag::capture_serde1(project).to_owned(),
        );
    }

    let target_overrides: UnorderedMap<String, OmniPath> =
        req.target.into_iter().collect();

    const GEN_DIR: &str = ".omni";
    const GEN_FILE: &str = ".omni/generator.json";
    let gen_output_dir = output_dir.join(GEN_DIR);
    let session_file = output_dir.join(GEN_FILE);

    let mut pre_exec_values = req.input_values;
    let mut target_overrides = target_overrides;

    // Restore saved session unless caller asked us to ignore it.
    let mut had_existing_session = false;
    if !req.ignore_session.unwrap_or(false)
        && sys.fs_exists_no_err_async(&session_file).await
    {
        let session =
            omni_generator::GenSession::from_disk(session_file.as_path(), &sys)
                .await?;
        had_existing_session = true;
        session
            .restore_targets(&name, &mut target_overrides, false)
            .await;
        session
            .restore_inputs_as_value_bag(&name, &mut pre_exec_values, false)
            .await;
    }

    let generators = get_generators(ctx.as_context(), &sys).await?;

    let run_config = RunConfig::builder()
        .dry_run(req.dry_run)
        .output_dir(output_dir.as_path())
        .maybe_overwrite(req.overwrite)
        .workspace_dir(&workspace_dir)
        .target_overrides(&target_overrides)
        .context_values(&context_values)
        .input_values(&pre_exec_values)
        .current_dir(&current_dir)
        .env(env.as_deref().unwrap_or(&default_map))
        .use_input_defaults(req.use_defaults)
        .available_generators(&generators)
        .input_provider(req.input_provider.as_ref())
        .subscriber(subscriber)
        .maybe_max_depth(req.max_depth)
        .build();

    let result = omni_generator::run_named(&name, &run_config, &sys).await?;

    let mut session_saved = false;

    if !req.dry_run
        && !result.session.is_empty().await
        && (!sys.fs_exists_no_err_async(session_file.as_path()).await
            || result
                .session
                .has_changes(session_file.as_path(), &sys)
                .await?)
        && (should_save_session(req.save_session, &req.input_provider).await?
            || had_existing_session)
    {
        if !sys.fs_exists_no_err_async(&gen_output_dir).await {
            sys.fs_create_dir_all_async(&gen_output_dir).await?;
        }
        result
            .session
            .write_to_disk(session_file.as_path(), &sys)
            .await?;

        session_saved = true;
    }

    Ok(GeneratorRunResponse {
        actions: result.actions,
        session_saved,
    })
}

async fn should_save_session(
    save_session: Option<bool>,
    input_provider: &Arc<dyn InputProvider<Generator>>,
) -> Result<bool, eyre::Error> {
    if let Some(save) = save_session {
        return Ok(save);
    }

    let prompt = boolean::<Generator>()
        .name("save_inputs")
        .base_extra(
            gen_base()
                .message("Would you like to save inputs and targets to the output directory?")
                .build(),
        )
        .default(true)
        .build();

    let result = collect_one(
        &prompt,
        None,
        &UnorderedMap::default(),
        &ValidationConfig::default(),
        input_provider.as_ref(),
    )
    .await?
    .expect("should have value");

    Ok(result.by_ref().to_bool().expect("should be bool"))
}

/// List all available generators in the workspace.
pub async fn handle_generator_list<TSys>(
    ctx: &Context<TSys>,
) -> eyre::Result<GeneratorListResponse>
where
    TSys: ContextSys + GeneratorSys + Clone,
{
    let sys = ctx.sys().clone();
    let generators = get_generators(ctx, &sys).await?;

    let generators = generators
        .iter()
        .filter_map(|g| {
            g.user_invocable.then(|| GeneratorInfo {
                name: g.name.clone(),
                display_name: g.display_name.clone(),
                description: g.description.clone(),
            })
        })
        .collect();

    Ok(GeneratorListResponse { generators })
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Discover and load all generators declared in the workspace configuration.
///
/// Mirrors the same logic in `omni_cli_core::commands::generator::get_generators`.
pub async fn get_generators<TSys>(
    ctx: &Context<TSys>,
    sys: &TSys,
) -> eyre::Result<Vec<Cow<'static, GeneratorConfiguration>>>
where
    TSys: ContextSys + GeneratorSys + Clone,
{
    let omni_path = ctx.omni_dir();
    let generator_sources_path = omni_path.join("./sources/generator");
    let lockfile_path = generator_sources_path.join("lock.json");

    sys.fs_create_dir_all_async(&generator_sources_path).await?;

    let remote_sources = Arc::new(
        RemoteSourceManager::new(
            RemoteSourceConfig::builder()
                .lockfile_path(lockfile_path)
                .soure_dir_path(generator_sources_path)
                .build(),
            sys.clone(),
        )
        .await?,
    );

    let mut retrieval_tasks: JoinSet<
        eyre::Result<Vec<Cow<'static, GeneratorConfiguration>>>,
    > = JoinSet::new();
    let mut git_sources = vec![];

    for (idx, config) in
        ctx.workspace_configuration().generators.iter().enumerate()
    {
        let scope_id = idx + 100;

        match config {
            GeneratorSourceConfiguration::Local(local) => {
                let local = local.clone();
                let root_dir = ctx.root_dir().to_path_buf();
                let sys = sys.clone();
                retrieval_tasks.spawn(async move {
                    let configurations = match local.path {
                        SingleOrMany::Single(item) => {
                            omni_generator::discover(&root_dir, &[item], &sys)
                                .await?
                        }
                        SingleOrMany::Many(items) => {
                            omni_generator::discover(&root_dir, &items, &sys)
                                .await?
                        }
                    };
                    Ok(omni_generator::assign_scope_id(
                        scope_id,
                        configurations,
                    ))
                });
            }
            GeneratorSourceConfiguration::Git(git) => {
                let remote_sources = remote_sources.clone();
                git_sources.push((&git.uri, git.rev.as_str()));
                let sys = sys.clone();
                let git = git.clone();
                retrieval_tasks.spawn(async move {
                    let dir = remote_sources
                        .pull_git_repo(&git.uri, &git.rev)
                        .await?;
                    let configurations =
                        omni_generator::discover(&dir, &["**"], &sys).await?;
                    Ok(omni_generator::assign_scope_id(
                        scope_id,
                        configurations,
                    ))
                });
            }
        }
    }

    let mut configurations = vec![];
    for configs in retrieval_tasks.join_all().await {
        configurations.extend(configs?);
    }

    remote_sources.retain_git_sources(&git_sources).await?;
    remote_sources.lock().await?;

    Ok(configurations)
}

// ── Generator Inspect view types ────────────────────────────────────────────

/// Determines which projection `generator_inspect` returns.
#[derive(
    Debug,
    Default,
    Serialize,
    Deserialize,
    JsonSchema,
    Clone,
    Copy,
    PartialEq,
    Eq,
)]
#[serde(rename_all = "kebab-case")]
pub enum InspectViewKind {
    /// Returns `Input<Generator>` translated to widget specs with inferred
    /// widget kinds. Default for backward compatibility.
    #[default]
    Widget,
    /// Returns `Input<()>` (data-only projection, no presentation extras).
    /// Used by MCP and other machine consumers.
    Data,
}

/// Trait implemented by the two inspect projections.
///
/// The associated type `NodeInputs` is the per-node input representation;
/// `render` converts a generator's raw `Input<Generator>` slice.
pub trait InspectView {
    type NodeInputs: Serialize
        + for<'de> Deserialize<'de>
        + JsonSchema
        + Debug
        + Clone;
    fn render(&self, inputs: &[Input<Generator>]) -> Self::NodeInputs;
}

/// Widget view: maps `Input<Generator>` to `Vec<GeneratorInputSpec>` using
/// the §5.6 widget inference table.
pub struct WidgetView;

impl InspectView for WidgetView {
    type NodeInputs = Vec<GeneratorInputSpec>;
    fn render(&self, inputs: &[Input<Generator>]) -> Vec<GeneratorInputSpec> {
        inputs.iter().map(input_to_spec).collect()
    }
}

/// Data view: maps `Input<Generator>` to `Vec<Input<()>>` by calling
/// `to_data()`, stripping all presentation extras.
pub struct DataView;

impl InspectView for DataView {
    type NodeInputs = Vec<InputSchema>;
    fn render(&self, inputs: &[Input<Generator>]) -> Vec<InputSchema> {
        inputs.iter().map(|i| i.to_data()).collect()
    }
}

/// A single node in the generator inspect tree, parameterised by how inputs
/// are projected (`N = Vec<GeneratorInputSpec>` for widgets, `N = Vec<Input<()>>`
/// for data).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GeneratorInspectNode<N> {
    pub name: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub inputs: N,
    pub targets: Vec<GeneratorTargetSpec>,
    /// Sub-generators invoked by `run-generator` actions, in declaration order.
    pub sub_generators: Vec<SubGeneratorRef<N>>,
}

/// View-tagged response returned by `generator_inspect`.
///
/// The `Widget` arm carries `GeneratorInputSpec` (widget kind, message,
/// options, remember). The `Data` arm carries `Input<()>` (data type,
/// allowed, secret, validators, condition — no presentation extras).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "view", content = "result", rename_all = "kebab-case")]
pub enum GeneratorInspectResponse {
    Widget(GeneratorInspectNode<Vec<GeneratorInputSpec>>),
    Data(GeneratorInspectNode<Vec<InputSchema>>),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GeneratorInputSpec {
    pub name: String,
    pub message: String,
    pub kind: GeneratorInputKind,
    pub required: bool,
    pub default: Option<InputDefault>,
    pub has_dynamic_default: bool,
    pub options: Vec<InputOption>,
    pub condition: Option<InputCondition>,
    pub validators: Vec<InputValidator>,
    pub remember: bool,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum GeneratorInputKind {
    Confirm,
    Select,
    MultiSelect,
    FreeEntryList,
    Text,
    Password,
    Float,
    Integer,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum InputDefault {
    Static { value: StaticInputDefault },
    Dynamic { expr: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum StaticInputDefault {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    IntList(Vec<i64>),
    FloatList(Vec<f64>),
    StrList(Vec<String>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum InputCondition {
    AlwaysHidden,
    Expression { expr: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InputValidator {
    pub condition: String,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InputOption {
    pub label: String,
    pub description: Option<String>,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GeneratorTargetSpec {
    pub key: String,
    pub default_path: String,
}

// ── Generator Inspect handler ─────────────────────────────────────────────────

/// Inspect a generator's full input schema, recursing into all sub-generators
/// invoked by `run-generator` actions. Cycle-safe.
///
/// `view` selects the input projection: `Widget` (default) for interactive
/// consumers, `Data` for machine consumers such as MCP.
pub async fn handle_generator_inspect<TSys>(
    ctx: &Context<TSys>,
    name: &str,
    view: InspectViewKind,
) -> eyre::Result<GeneratorInspectResponse>
where
    TSys: ContextSys + GeneratorSys + Clone,
{
    let sys = ctx.sys().clone();
    let generators = get_generators(ctx, &sys).await?;
    let mut visited = HashSet::new();
    match view {
        InspectViewKind::Widget => {
            inspect_tree(name, &WidgetView, &generators, &mut visited)
                .map(GeneratorInspectResponse::Widget)
                .ok_or_else(|| eyre::eyre!("generator '{}' not found", name))
        }
        InspectViewKind::Data => {
            inspect_tree(name, &DataView, &generators, &mut visited)
                .map(GeneratorInspectResponse::Data)
                .ok_or_else(|| eyre::eyre!("generator '{}' not found", name))
        }
    }
}

fn omni_path_to_string(path: &OmniPath) -> String {
    if path.is_any_rooted() {
        format!(
            "@{}/{}",
            path.root().expect("root should be set"),
            path.unresolved_path().to_string_lossy()
        )
    } else {
        path.unresolved_path().to_string_lossy().to_string()
    }
}

/// Map `Input<Generator>` to `GeneratorInputSpec`
fn input_to_spec(input: &Input<Generator>) -> GeneratorInputSpec {
    let base = input.base();
    let gen_base: &GenBase = input.base_extra();
    let name = base.name.clone();
    let message = gen_base.message.clone();
    let remember = gen_base.remember;
    let condition = condition_from_if(base.r#if.as_ref());
    let description = base.description.clone();
    let validators = validators_from_base(base);

    let (kind, default, options) = match input {
        Input::Boolean(b) => (
            GeneratorInputKind::Confirm,
            b.default
                .as_ref()
                .and_then(MaybeExpr::try_as_value_ref)
                .map(|v| InputDefault::Static {
                    value: StaticInputDefault::Bool(*v),
                }),
            vec![],
        ),
        Input::String(s) => {
            let kind = match s.string_extra.widget {
                Some(StringWidget::Password) => GeneratorInputKind::Password,
                Some(StringWidget::Select) => GeneratorInputKind::Select,
                Some(StringWidget::Text) | None => {
                    if base.secret {
                        GeneratorInputKind::Password
                    } else if s.allowed.is_some() {
                        GeneratorInputKind::Select
                    } else {
                        GeneratorInputKind::Text
                    }
                }
            };
            let options: Vec<InputOption> = s
                .allowed
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(string_allowed_to_option)
                .collect();
            let default = s.default.as_ref().map(|v| InputDefault::Static {
                value: StaticInputDefault::Str(v.clone()),
            });
            (kind, default, options)
        }
        Input::Integer(i) => {
            let kind = if i.allowed.is_some() {
                GeneratorInputKind::Select
            } else {
                GeneratorInputKind::Integer
            };
            let options: Vec<InputOption> = i
                .allowed
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(int_allowed_to_option)
                .collect();
            let default = i
                .default
                .as_ref()
                .and_then(MaybeExpr::try_as_value_ref)
                .map(|v| InputDefault::Static {
                    value: StaticInputDefault::Int(*v),
                });
            (kind, default, options)
        }
        Input::Float(f) => {
            let kind = if f.allowed.is_some() {
                GeneratorInputKind::Select
            } else {
                GeneratorInputKind::Float
            };
            let options: Vec<InputOption> = f
                .allowed
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(float_allowed_to_option)
                .collect();
            let default = f
                .default
                .as_ref()
                .and_then(MaybeExpr::try_as_value_ref)
                .map(|v| InputDefault::Static {
                    value: StaticInputDefault::Float(*v),
                });
            (kind, default, options)
        }
        Input::StringArray(sa) => {
            let kind = match sa.array_extra.widget {
                Some(ListWidget::FreeEntry) => {
                    GeneratorInputKind::FreeEntryList
                }
                Some(ListWidget::MultiSelect) | None => {
                    if sa.body.allowed.is_some() {
                        GeneratorInputKind::MultiSelect
                    } else {
                        GeneratorInputKind::FreeEntryList
                    }
                }
            };
            let options: Vec<InputOption> = sa
                .body
                .allowed
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(string_allowed_to_option)
                .collect();
            let default = sa.default.as_ref().map(|v| InputDefault::Static {
                value: StaticInputDefault::StrList(v.clone()),
            });
            (kind, default, options)
        }
        Input::IntegerArray(ia) => {
            let kind = match ia.array_extra.widget {
                Some(ListWidget::FreeEntry) => {
                    GeneratorInputKind::FreeEntryList
                }
                Some(ListWidget::MultiSelect) | None => {
                    if ia.body.allowed.is_some() {
                        GeneratorInputKind::MultiSelect
                    } else {
                        GeneratorInputKind::FreeEntryList
                    }
                }
            };
            let options: Vec<InputOption> = ia
                .body
                .allowed
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(int_allowed_to_option)
                .collect();
            let default = ia.default.as_ref().map(|v| InputDefault::Static {
                value: StaticInputDefault::IntList(v.clone()),
            });
            (kind, default, options)
        }
        Input::FloatArray(fa) => {
            let kind = match fa.array_extra.widget {
                Some(ListWidget::FreeEntry) => {
                    GeneratorInputKind::FreeEntryList
                }
                Some(ListWidget::MultiSelect) | None => {
                    if fa.body.allowed.is_some() {
                        GeneratorInputKind::MultiSelect
                    } else {
                        GeneratorInputKind::FreeEntryList
                    }
                }
            };
            let options: Vec<InputOption> = fa
                .body
                .allowed
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(float_allowed_to_option)
                .collect();
            let default = fa.default.as_ref().map(|v| InputDefault::Static {
                value: StaticInputDefault::FloatList(v.clone()),
            });
            (kind, default, options)
        }
        Input::Object(_) => {
            // Object is excluded from Generator::SUPPORTED; this arm is
            // unreachable in practice but required for exhaustive match.
            (GeneratorInputKind::Text, None, vec![])
        }
    };

    let has_dynamic_default = input.dynamic_default_expr().is_some();

    let required = default.is_none()
        && !has_dynamic_default
        && !matches!(&condition, Some(InputCondition::AlwaysHidden));

    GeneratorInputSpec {
        description,
        name,
        message,
        kind,
        required,
        default,
        has_dynamic_default,
        options,
        condition,
        validators,
        remember,
    }
}

fn condition_from_if(
    if_expr: Option<&MaybeExpr<bool>>,
) -> Option<InputCondition> {
    match if_expr? {
        MaybeExpr::Value(true) => None,
        MaybeExpr::Value(false) => Some(InputCondition::AlwaysHidden),
        MaybeExpr::Expr(expr) => Some(InputCondition::Expression {
            expr: expr.to_string(),
        }),
    }
}

fn validators_from_base(base: &BaseInput) -> Vec<InputValidator> {
    base.validators
        .iter()
        .map(|v| InputValidator {
            condition: match &v.condition {
                MaybeExpr::Value(b) => b.to_string(),
                MaybeExpr::Expr(expr) => expr.to_string(),
            },
            error_message: v.error_message.clone(),
        })
        .collect()
}

fn string_allowed_to_option(
    opt: &AllowedValue<String, Generator>,
) -> InputOption {
    InputOption {
        label: opt
            .base_extra
            .name
            .clone()
            .unwrap_or_else(|| opt.value.clone()),
        description: opt.description.clone(),
        value: opt.value.clone(),
    }
}

fn int_allowed_to_option(opt: &AllowedValue<i64, Generator>) -> InputOption {
    InputOption {
        label: opt
            .base_extra
            .name
            .clone()
            .unwrap_or_else(|| opt.value.to_string()),
        description: opt.description.clone(),
        value: opt.value.to_string(),
    }
}

fn float_allowed_to_option(opt: &AllowedValue<f64, Generator>) -> InputOption {
    InputOption {
        label: opt
            .base_extra
            .name
            .clone()
            .unwrap_or_else(|| opt.value.to_string()),
        description: opt.description.clone(),
        value: opt.value.to_string(),
    }
}

// ── Validate Input types ──────────────────────────────────────────────────────

/// Request to validate a set of input values against a generator's schema.
#[derive(Debug, JsonSchema)]
pub struct GeneratorValidateInputRequest {
    pub name: String,
    #[schemars(with = "std::collections::HashMap<String, serde_json::Value>")]
    pub input_values: UnorderedMap<String, OwnedValueBag>,
    /// When `true`, inputs that have a default are not flagged as missing.
    /// Mirrors the behaviour of `generator_run`'s `use_defaults` flag.
    pub use_defaults: bool,
}

/// Validation result for a single sub-generator invoked by a `run-generator` action.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SubGeneratorValidationResult {
    pub generator_name: String,
    /// The `if` expression on the `run-generator` action, if any.
    pub action_condition: Option<String>,
    pub valid: bool,
    pub errors: Vec<InputFieldError>,
    pub sub_generators: Vec<SubGeneratorValidationResult>,
}

/// Response from a `generator_validate_input` call.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GeneratorValidateInputResponse {
    /// `true` when all errors (including sub-generators) are empty.
    pub valid: bool,
    pub errors: Vec<InputFieldError>,
    pub sub_generators: Vec<SubGeneratorValidationResult>,
}

/// A validation error for a single named input field.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct InputFieldError {
    pub input_name: String,
    pub message: String,
}

// ── Validate Input handler ────────────────────────────────────────────────────

/// Validate a set of input values against a generator's full schema, including
/// all sub-generators invoked by `run-generator` actions. Cycle-safe.
pub async fn handle_generator_validate_input<TSys>(
    ctx: &Context<TSys>,
    req: GeneratorValidateInputRequest,
) -> eyre::Result<GeneratorValidateInputResponse>
where
    TSys: ContextSys + GeneratorSys + Clone,
{
    let sys = ctx.sys().clone();
    let generators = get_generators(ctx, &sys).await?;

    let config = omni_input_provider::ValidationConfig {
        use_defaults: req.use_defaults,
        ..Default::default()
    };

    let mut visited = HashSet::new();
    let (errors, sub_generators) = validate_generator(
        &req.name,
        &req.input_values,
        &generators,
        &config,
        &mut visited,
    )
    .ok_or_else(|| eyre::eyre!("generator '{}' not found", req.name))?;

    let valid = errors.is_empty() && sub_generators.iter().all(|s| s.valid);
    Ok(GeneratorValidateInputResponse {
        valid,
        errors,
        sub_generators,
    })
}

fn validate_generator(
    name: &str,
    input_values: &UnorderedMap<String, OwnedValueBag>,
    generators: &[Cow<'static, GeneratorConfiguration>],
    config: &omni_input_provider::ValidationConfig<'_>,
    visited: &mut HashSet<String>,
) -> Option<(Vec<InputFieldError>, Vec<SubGeneratorValidationResult>)> {
    let generator = generators.iter().find(|g| g.name == name)?;

    let report = match omni_input_provider::validate(
        &generator.inputs,
        input_values,
        &Default::default(),
        config,
    ) {
        Ok(r) => r,
        Err(e) => {
            return Some((
                vec![InputFieldError {
                    input_name: String::from("_configuration"),
                    message: e.to_string(),
                }],
                vec![],
            ));
        }
    };

    let errors: Vec<InputFieldError> = report
        .errors
        .into_iter()
        .map(|e| InputFieldError {
            input_name: e.input_name,
            message: e.message,
        })
        .collect();

    visited.insert(name.to_string());

    let sub_generators = generator
        .actions
        .iter()
        .filter_map(|action| {
            let ActionConfiguration::RunGenerator { action } = action else {
                return None;
            };
            if visited.contains(&action.generator) {
                return None;
            }
            let effective_inputs = compute_effective_sub_inputs(
                &action.input_values,
                input_values,
            );
            let (sub_errors, sub_sub) = validate_generator(
                &action.generator,
                &effective_inputs,
                generators,
                config,
                visited,
            )?;
            let valid =
                sub_errors.is_empty() && sub_sub.iter().all(|s| s.valid);
            Some(SubGeneratorValidationResult {
                generator_name: action.generator.clone(),
                action_condition: action.base.r#if.clone(),
                valid,
                errors: sub_errors,
                sub_generators: sub_sub,
            })
        })
        .collect();

    visited.remove(name);

    Some((errors, sub_generators))
}

/// Compute the effective input values a sub-generator will receive:
/// forwarded parent inputs are applied first, then pre-filled static values
/// from the action config override them.
fn compute_effective_sub_inputs(
    cfg: &omni_generator_configurations::InputValuesConfiguration,
    parent_inputs: &UnorderedMap<String, OwnedValueBag>,
) -> UnorderedMap<String, OwnedValueBag> {
    let mut effective = UnorderedMap::default();

    match &cfg.forward {
        ForwardInputValuesConfiguration::ForAll(
            ForAllInputValuesConfiguration::All,
        ) => effective.extend(parent_inputs.clone()),
        ForwardInputValuesConfiguration::Selected(names) => {
            for name in names {
                if let Some(v) = parent_inputs.get(name.as_str()) {
                    effective.insert(name.clone(), v.clone());
                }
            }
        }
        ForwardInputValuesConfiguration::ForAll(
            ForAllInputValuesConfiguration::None,
        ) => {}
    }

    for (k, v) in &cfg.values {
        effective.insert(k.clone(), input_value_to_owned_value_bag(v));
    }

    effective
}

fn input_value_to_owned_value_bag(v: &InputValue) -> OwnedValueBag {
    ValueBag::from_serde1(&input_value_to_json(v)).to_owned()
}

// ── Sub-generator traversal types ──────────────────────────────────────────────────

/// Describes how the parent generator's inputs flow into a sub-generator.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum ForwardedInputs {
    /// All parent inputs are forwarded into the sub-generator's context.
    All,
    /// No parent inputs are forwarded.
    None,
    /// Only the named parent inputs are forwarded.
    Selected { names: Vec<String> },
}

/// An invocation of a sub-generator found inside a `run-generator` action.
///
/// Generic over `N` (the same type used by `GeneratorInspectNode`) so the
/// widget and data views can share the recursive tree walk.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubGeneratorRef<N> {
    /// The generator name as written in the config.
    pub name: String,
    /// The `if` expression on the `run-generator` action, if any.
    pub action_condition: Option<String>,
    /// Which parent inputs flow into the sub-generator's context automatically.
    pub forwarded_inputs: ForwardedInputs,
    /// Inputs that are pre-set with static values in the action config (key → JSON value).
    pub pre_filled_inputs: Vec<(String, serde_json::Value)>,
    /// Recursive inspect result; `None` when a cycle was detected.
    pub generator: Option<Box<GeneratorInspectNode<N>>>,
}

fn inspect_tree<V: InspectView>(
    name: &str,
    view: &V,
    generators: &[Cow<'static, GeneratorConfiguration>],
    visited: &mut HashSet<String>,
) -> Option<GeneratorInspectNode<V::NodeInputs>> {
    let generator = generators.iter().find(|g| g.name == name)?;

    let inputs = view.render(&generator.inputs);
    let targets = generator
        .targets
        .iter()
        .map(|(key, path)| GeneratorTargetSpec {
            key: key.clone(),
            default_path: omni_path_to_string(path),
        })
        .collect();

    visited.insert(name.to_string());

    let sub_generators = generator
        .actions
        .iter()
        .filter_map(|action| {
            let ActionConfiguration::RunGenerator { action } = action else {
                return None;
            };
            let action_condition = action.base.r#if.clone();

            let forwarded_inputs = match &action.input_values.forward {
                ForwardInputValuesConfiguration::ForAll(
                    ForAllInputValuesConfiguration::All,
                ) => ForwardedInputs::All,
                ForwardInputValuesConfiguration::ForAll(
                    ForAllInputValuesConfiguration::None,
                ) => ForwardedInputs::None,
                ForwardInputValuesConfiguration::Selected(names) => {
                    ForwardedInputs::Selected {
                        names: names.clone(),
                    }
                }
            };

            let pre_filled_inputs = action
                .input_values
                .values
                .iter()
                .map(|(k, v)| (k.clone(), input_value_to_json(v)))
                .collect();

            let child_generator = if visited.contains(&action.generator) {
                None
            } else {
                inspect_tree(&action.generator, view, generators, visited)
                    .map(Box::new)
            };

            Some(SubGeneratorRef {
                name: action.generator.clone(),
                action_condition,
                forwarded_inputs,
                pre_filled_inputs,
                generator: child_generator,
            })
        })
        .collect();

    visited.remove(name);

    Some(GeneratorInspectNode {
        name: generator.name.clone(),
        display_name: generator.display_name.clone(),
        description: generator.description.clone(),
        inputs,
        targets,
        sub_generators,
    })
}

fn input_value_to_json(v: &InputValue) -> serde_json::Value {
    match v {
        InputValue::Integer(i) => serde_json::Value::Number((*i).into()),
        InputValue::Float(f) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        InputValue::Boolean(b) => serde_json::Value::Bool(*b),
        InputValue::String(s) => serde_json::Value::String(s.clone()),
        InputValue::List(l) => serde_json::Value::Array(
            l.iter().map(input_value_to_json).collect(),
        ),
    }
}
