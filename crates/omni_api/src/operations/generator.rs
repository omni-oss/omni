use either::Either;
use either::Either::Left;
use omni_generator::Action;
use omni_generator_configurations::ActionConfiguration;
use omni_generator_configurations::ForAllInputValuesConfiguration;
use omni_generator_configurations::ForwardInputValuesConfiguration;
use omni_generator_configurations::GeneratorConfiguration;
use omni_generator_configurations::InputConfigurationExtra;
use omni_generator_configurations::InputValue;
use omni_generator_configurations::OmniPath;
use omni_generator_configurations::OverwriteConfiguration;
use omni_input_provider::BaseInputConfiguration;
use omni_input_provider::ConfirmInputConfiguration;
use omni_input_provider::InputConfiguration;
use omni_input_provider::InputProvider;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashSet;
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
    pub input_provider: Arc<dyn InputProvider>,
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

    let run_config = RunConfig {
        dry_run: req.dry_run,
        output_dir: output_dir.as_path(),
        overwrite: req.overwrite,
        workspace_dir: &workspace_dir,
        target_overrides: &target_overrides,
        context_values: &context_values,
        input_values: &pre_exec_values,
        current_dir: &current_dir,
        env: env.as_deref().unwrap_or(&default_map),
        args: None,
        use_input_defaults: req.use_defaults,
        available_generators: &generators,
        input_provider: req.input_provider.as_ref(),
        subscriber,
        max_depth: req
            .max_depth
            .unwrap_or(omni_generator::DEFAULT_MAX_GENERATOR_DEPTH),
    };

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
    input_provider: &Arc<dyn InputProvider>,
) -> Result<bool, eyre::Error> {
    Ok(if let Some(save) = save_session {
        save
    } else {
        let config = ConfirmInputConfiguration::new(
            BaseInputConfiguration::new(
                "save_inputs",
                "Would you like to save inputs and targets to the output directory?",
                Some(Left(true)),
                None,
            ),
            Some(Left(true)),
        );
        input_provider
            .confirm(&config, &omni_tera::Context::default())
            .await?
    })
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

// ── Generator Inspect types ──────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GeneratorInspectResponse {
    pub name: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub inputs: Vec<GeneratorInputSpec>,
    pub targets: Vec<GeneratorTargetSpec>,
    /// Sub-generators invoked by `run-generator` actions, in declaration order.
    pub sub_generators: Vec<SubGeneratorRef>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
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

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum GeneratorInputKind {
    Confirm,
    Select,
    MultiSelect,
    Text,
    Password,
    Float,
    Integer,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum InputDefault {
    Static { value: StaticInputDefault },
    Dynamic { expr: String },
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum StaticInputDefault {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    StrList(Vec<String>),
}

#[derive(Debug, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum InputCondition {
    AlwaysHidden,
    Expression { expr: String },
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct InputValidator {
    pub condition: String,
    pub error_message: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct InputOption {
    pub label: String,
    pub description: Option<String>,
    pub value: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GeneratorTargetSpec {
    pub key: String,
    pub default_path: String,
}

// ── Generator Inspect handler ─────────────────────────────────────────────────

/// Inspect a generator's full input schema, recursing into all sub-generators
/// invoked by `run-generator` actions. Cycle-safe.
pub async fn handle_generator_inspect<TSys>(
    ctx: &Context<TSys>,
    name: &str,
) -> eyre::Result<GeneratorInspectResponse>
where
    TSys: ContextSys + GeneratorSys + Clone,
{
    let sys = ctx.sys().clone();
    let generators = get_generators(ctx, &sys).await?;
    let mut visited = HashSet::new();
    inspect_generator(name, &generators, &mut visited)
        .ok_or_else(|| eyre::eyre!("generator '{}' not found", name))
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

fn translate_input(
    input: &InputConfiguration<InputConfigurationExtra>,
) -> GeneratorInputSpec {
    let name = input.name().to_string();
    let message = input.message().to_string();
    let remember = input.extra().remember;
    let condition = translate_condition(input.condition());
    let description = input.description().map(|s| s.to_owned());

    let (kind, default, options, validators) = match input {
        InputConfiguration::Confirm { input, .. } => (
            GeneratorInputKind::Confirm,
            either_to_default_bool(input.default.as_ref()),
            vec![],
            vec![],
        ),
        InputConfiguration::Select { input, .. } => (
            GeneratorInputKind::Select,
            input.default.as_ref().map(|s| InputDefault::Static {
                value: StaticInputDefault::Str(s.clone()),
            }),
            input.options.iter().map(translate_option).collect(),
            vec![],
        ),
        InputConfiguration::MultiSelect { input, .. } => (
            GeneratorInputKind::MultiSelect,
            input.default.as_ref().map(|v| InputDefault::Static {
                value: StaticInputDefault::StrList(v.clone()),
            }),
            input.options.iter().map(translate_option).collect(),
            vec![],
        ),
        InputConfiguration::Text { input, .. } => (
            GeneratorInputKind::Text,
            input.default.as_ref().map(|s| InputDefault::Static {
                value: StaticInputDefault::Str(s.clone()),
            }),
            vec![],
            translate_validators(&input.base.validate),
        ),
        InputConfiguration::Password { input, .. } => (
            GeneratorInputKind::Password,
            None,
            vec![],
            translate_validators(&input.base.validate),
        ),
        InputConfiguration::Float { input, .. } => (
            GeneratorInputKind::Float,
            either_to_default_float(input.default.as_ref()),
            vec![],
            translate_validators(&input.base.validate),
        ),
        InputConfiguration::Integer { input, .. } => (
            GeneratorInputKind::Integer,
            either_to_default_int(input.default.as_ref()),
            vec![],
            translate_validators(&input.base.validate),
        ),
    };

    let has_dynamic_default =
        matches!(&default, Some(InputDefault::Dynamic { .. }));
    let required = default.is_none()
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

fn translate_condition(
    cond: Option<&Either<bool, String>>,
) -> Option<InputCondition> {
    match cond? {
        Either::Left(true) => None,
        Either::Left(false) => Some(InputCondition::AlwaysHidden),
        Either::Right(expr) => {
            Some(InputCondition::Expression { expr: expr.clone() })
        }
    }
}

fn either_to_default_bool(
    v: Option<&Either<bool, String>>,
) -> Option<InputDefault> {
    Some(match v? {
        Either::Left(b) => InputDefault::Static {
            value: StaticInputDefault::Bool(*b),
        },
        Either::Right(expr) => InputDefault::Dynamic { expr: expr.clone() },
    })
}

fn either_to_default_int(
    v: Option<&Either<i64, String>>,
) -> Option<InputDefault> {
    Some(match v? {
        Either::Left(i) => InputDefault::Static {
            value: StaticInputDefault::Int(*i),
        },
        Either::Right(expr) => InputDefault::Dynamic { expr: expr.clone() },
    })
}

fn either_to_default_float(
    v: Option<&Either<f64, String>>,
) -> Option<InputDefault> {
    Some(match v? {
        Either::Left(f) => InputDefault::Static {
            value: StaticInputDefault::Float(*f),
        },
        Either::Right(expr) => InputDefault::Dynamic { expr: expr.clone() },
    })
}

fn translate_validators(
    validators: &[omni_input_provider::ValidateConfiguration],
) -> Vec<InputValidator> {
    validators
        .iter()
        .map(|v| InputValidator {
            condition: match &v.condition {
                Either::Left(b) => b.to_string(),
                Either::Right(expr) => expr.clone(),
            },
            error_message: v.error_message.clone(),
        })
        .collect()
}

fn translate_option(
    opt: &omni_input_provider::OptionConfiguration,
) -> InputOption {
    InputOption {
        label: opt.name.clone(),
        description: opt.description.clone(),
        value: opt.value.clone(),
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

    let config = omni_input_provider::CollectionConfig {
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
    config: &omni_input_provider::CollectionConfig<'_>,
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

// ── Sub-generator traversal types ───────────────────────────────────────────

/// Describes how the parent generator's inputs flow into a sub-generator.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SubGeneratorRef {
    /// The generator name as written in the config.
    pub name: String,
    /// The `if` expression on the `run-generator` action, if any.
    pub action_condition: Option<String>,
    /// Which parent inputs flow into the sub-generator's context automatically.
    pub forwarded_inputs: ForwardedInputs,
    /// Inputs that are pre-set with static values in the action config (key → JSON value).
    pub pre_filled_inputs: Vec<(String, serde_json::Value)>,
    /// Recursive inspect result; `None` when a cycle was detected.
    pub generator: Option<Box<GeneratorInspectResponse>>,
}

fn inspect_generator(
    name: &str,
    generators: &[Cow<'static, GeneratorConfiguration>],
    visited: &mut HashSet<String>,
) -> Option<GeneratorInspectResponse> {
    let generator = generators.iter().find(|g| g.name == name)?;

    let inputs = generator.inputs.iter().map(translate_input).collect();
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
                inspect_generator(&action.generator, generators, visited)
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

    Some(GeneratorInspectResponse {
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
