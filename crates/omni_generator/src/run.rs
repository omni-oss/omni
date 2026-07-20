use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

use maps::{Map, UnorderedMap, unordered_map};
use omni_capabilities::{CapabilityRules, CapabilitiesStrictness};
use omni_generator_configurations::{
    Generator, GeneratorConfiguration, OmniPath, OverwriteConfiguration,
};
use omni_input_provider::{
    InputProfile, InputProvider, ValidationConfig, collect,
};
use omni_messages::{
    GeneratorCompletedEvent, GeneratorEventSubscriber, GeneratorStartEvent,
    NoopSubscriber,
};
use serde::{Deserialize, Serialize};
use sets::UnorderedSet;
use strum::EnumDiscriminants;
use value_bag::{OwnedValueBag, ValueBag};

use omni_utils::lock::LockGuard;

use crate::{
    GeneratorSys, GeneratorSysFull, JsScriptRunner, LazyScriptRunner,
    error::{Error, ErrorInner},
    execute_actions::{ExecuteActionsArgs, execute_actions},
    gen_session::GenSession,
    sys_impl::{self, PendingActionsVisitor, TransactionSys},
    utils::{expand_json_value, get_tera_context},
};

/// Default cap on `run-generator` nesting depth. The static `detect_recursion`
/// pass is the primary cycle guard; this bound is a defense-in-depth backstop
/// for runtime edges the static graph cannot model. Real nesting is only a few
/// levels deep, so the default is generous. Callers can override it via
/// [`RunConfig::max_depth`] when a config legitimately nests deeper.
pub const DEFAULT_MAX_GENERATOR_DEPTH: usize = 64;

/// The empty workspace-capability floor used when a caller does not supply one
/// (preserving the opt-in-per-generator behaviour). A workspace that declares a
/// floor passes it explicitly via [`RunConfig::workspace_capabilities`].
static EMPTY_WORKSPACE_CAPABILITIES: CapabilityRules<Generator> =
    CapabilityRules(Vec::new());

#[derive(Debug, bon::Builder)]
pub struct RunConfig<'a, S: GeneratorEventSubscriber = NoopSubscriber> {
    #[builder(default)]
    pub dry_run: bool,
    pub output_dir: &'a Path,
    pub overwrite: Option<OverwriteConfiguration>,
    pub workspace_dir: &'a Path,
    pub current_dir: &'a Path,
    pub target_overrides: &'a UnorderedMap<String, OmniPath>,
    pub input_values: &'a UnorderedMap<String, OwnedValueBag>,
    pub context_values: &'a UnorderedMap<String, OwnedValueBag>,
    pub env: &'a Map<String, String>,
    pub args: Option<&'a UnorderedMap<String, serde_json::Value>>,
    #[builder(default)]
    pub use_input_defaults: bool,
    pub available_generators: &'a [Cow<'a, GeneratorConfiguration>],
    /// Workspace-level capability floor (already upcast to the generator
    /// profile). Folded ahead of each generator's own policy and forwarded to
    /// nested generators, so a workspace `deny` cannot be re-opened downstream.
    /// Defaults to empty (opt-in per generator).
    #[builder(default = &EMPTY_WORKSPACE_CAPABILITIES)]
    pub workspace_capabilities: &'a CapabilityRules<Generator>,
    /// Workspace-level floor-gap strictness stance. Seeds the accumulated
    /// strictness for the top-level generator and every nested run, combined
    /// most-severe with each generator's and action's own stance. Defaults to
    /// [`CapabilitiesStrictness::Warn`].
    #[builder(default)]
    pub workspace_strictness: CapabilitiesStrictness,
    pub input_provider: &'a dyn InputProvider<Generator>,
    pub subscriber: &'a S,
    /// Maximum `run-generator` nesting depth before a run is aborted. Use
    /// [`DEFAULT_MAX_GENERATOR_DEPTH`] unless a config legitimately nests deeper.
    #[builder(default = DEFAULT_MAX_GENERATOR_DEPTH)]
    pub max_depth: usize,
}

pub struct GeneratorRunResult {
    pub session: GenSession,
    pub actions: Vec<Action>,
}

pub async fn run_named<'a, S: GeneratorEventSubscriber>(
    generator_name: &'a str,
    config: &RunConfig<'a, S>,
    sys: &impl GeneratorSys,
) -> Result<GeneratorRunResult, Error> {
    crate::validate(config.available_generators)?;

    let generator = config
        .available_generators
        .iter()
        .find(|g| g.name == generator_name)
        .ok_or_else(|| {
            ErrorInner::new_generator_not_found(generator_name.to_string())
        })?;

    run_in_transaction(&generator, config, sys).await
}

pub async fn run<'a, S: GeneratorEventSubscriber>(
    generator: &'a GeneratorConfiguration,
    config: &RunConfig<'a, S>,
    sys: &impl GeneratorSys,
) -> Result<GeneratorRunResult, Error> {
    crate::validate(config.available_generators)?;
    run_in_transaction(generator, config, sys).await
}

/// Runs a generator against a transactional overlay of `sys`.
///
/// Every file-system mutation performed while executing the generator's
/// actions is buffered in the overlay; nothing touches `sys` until the
/// transaction is committed. A normal run commits the transaction once all
/// actions have succeeded (making generation atomic), while a dry run simply
/// drops the buffered actions without committing.
async fn run_in_transaction<'a, S: GeneratorEventSubscriber>(
    generator: &GeneratorConfiguration,
    config: &RunConfig<'a, S>,
    sys: &impl GeneratorSys,
) -> Result<GeneratorRunResult, Error> {
    if !generator.user_invocable {
        return Err(ErrorInner::new_generator_not_invocable(
            generator.name.to_string(),
        )
        .into());
    }

    // The workspace floor must itself be enforceable (e.g. no `net` rule, which
    // generators cannot express). Validate once here; nested generators reuse
    // the same already-validated floor.
    omni_capabilities::validate(config.workspace_capabilities)
        .map_err(ErrorInner::new_invalid_workspace_capabilities)?;

    crate::detect_recursion(generator, config.available_generators)?;

    // Serialize all generator runs within the same workspace. Generators can
    // write to arbitrary workspace-level paths (not just output_dir), so
    // per-directory locking is insufficient — a single workspace-scoped lock
    // is the only safe granularity.
    let lock_path = config
        .workspace_dir
        .join(".omni")
        .join("locks")
        .join("generator.lock");
    let _lock = LockGuard::acquire_exclusive(lock_path).await?;

    let tx = TransactionSys::new(sys.clone());
    let runner = LazyScriptRunner::new(
        tx.clone(),
        config.workspace_dir.to_path_buf(),
        env!("CARGO_PKG_VERSION").to_string(),
    );
    config
        .subscriber
        .on_generator_start(GeneratorStartEvent {
            name: generator.name.clone(),
        })
        .await;

    let result = run_internal(
        generator,
        config,
        &tx,
        &runner,
        0,
        std::slice::from_ref(config.workspace_capabilities),
        config.workspace_strictness,
    )
    .await;

    // Tear down the JS process (if one was started) regardless of outcome.
    runner.shutdown().await;

    let session = result?;

    let actions = {
        struct InferActionsVisitor {
            actions: Vec<Action>,
        }
        impl<TSys: GeneratorSys> PendingActionsVisitor<TSys> for InferActionsVisitor {
            type Error = eyre::Report;

            async fn visit_action(
                &mut self,
                action: &sys_impl::Action,
                sys: &TSys,
            ) -> Result<(), Self::Error> {
                if let Some(a) = Action::infer_from(action, sys).await? {
                    self.actions.push(a);
                }
                Ok(())
            }
        }

        let mut visitor = InferActionsVisitor {
            actions: Vec::new(),
        };
        tx.visit_pending_actions(&mut visitor).await?;
        visitor.actions
    };

    if !config.dry_run {
        tx.commit().await?;
    }

    config
        .subscriber
        .on_generator_completed(GeneratorCompletedEvent {
            name: generator.name.clone(),
        })
        .await;

    Ok(GeneratorRunResult { session, actions })
}

pub(crate) async fn run_internal<'a, S: GeneratorEventSubscriber>(
    r#gen: &GeneratorConfiguration,
    config: &RunConfig<'a, S>,
    sys: &impl GeneratorSysFull,
    runner: &dyn JsScriptRunner,
    depth: usize,
    inherited_capabilities: &[CapabilityRules<Generator>],
    inherited_strictness: CapabilitiesStrictness,
) -> Result<GenSession, Error> {
    if depth > config.max_depth {
        return Err(ErrorInner::new_max_generator_depth_exceeded(
            r#gen.name.clone(),
            config.max_depth,
        )
        .into());
    }

    let collection_config = ValidationConfig {
        use_defaults: config.use_input_defaults,
        ..ValidationConfig::default()
    };

    let session = GenSession::with_restored(
        r#gen.name.as_str(),
        config.target_overrides.clone(),
        Default::default(),
    );

    let mut values = collect(
        &r#gen.inputs,
        &config.input_values,
        &config.context_values,
        &collection_config,
        config.input_provider,
    )
    .await?;

    // propagate prompt values to the context values
    for (key, value) in config.input_values.iter() {
        if !values.contains_key(key) {
            values.insert(key.to_string(), value.clone());
        }
    }

    trace::trace!(?values, "input_values");

    let mut context_values = config.context_values.clone();

    context_values.insert(
        "inputs".to_string(),
        ValueBag::capture_serde1(&values).to_owned(),
    );

    let vars = expand_vars("vars", &r#gen.vars, &context_values)?;

    context_values.insert(
        "vars".to_string(),
        ValueBag::capture_serde1(&vars).to_owned(),
    );

    if let Some(args) = config.args {
        let args = expand_vars("args", args, &context_values)?;
        context_values.insert(
            "args".to_string(),
            ValueBag::capture_serde1(&args).to_owned(),
        );
    } else {
        let map = UnorderedMap::<&str, ()>::default();
        context_values.insert(
            "args".to_string(),
            ValueBag::capture_serde1(&map).to_owned(),
        );
    }

    let args = ExecuteActionsArgs {
        actions: &r#gen.actions,
        context_values: &context_values,
        dry_run: config.dry_run,
        output_dir: config.output_dir,
        generator_dir: &r#gen
            .config_path
            .parent()
            .expect("generator should have a directory"),
        generator_name: &r#gen.name,
        scope_id: r#gen.scope_id.as_deref(),
        targets: &r#gen.targets,
        overwrite: config.overwrite,
        available_generators: config.available_generators,
        target_overrides: config.target_overrides,
        current_dir: config.current_dir,
        env: config.env,
        workspace_dir: config.workspace_dir,
        workspace_capabilities: config.workspace_capabilities,
        inherited_capabilities,
        capabilities: &r#gen.capabilities.rules,
        // Effective floor-gap stance for this generator: the most-severe of
        // everything inherited (workspace ⟺ ancestor generators) and this
        // generator's own declared stance. `run-javascript` combines the
        // action's stance on top of this.
        capabilities_strictness: inherited_strictness
            .max(r#gen.capabilities.strictness),
        js_script_runner: runner,
        input_provider: config.input_provider,
        subscriber: config.subscriber,
        use_input_defaults: config.use_input_defaults,
        depth,
        max_depth: config.max_depth,
    };

    execute_actions(&args, &session, sys).await?;

    let skip = r#gen
        .inputs
        .iter()
        .filter_map(|p| {
            if Generator::is_remember(p) {
                None
            } else {
                Some(p.base().name.as_str())
            }
        })
        .collect::<UnorderedSet<_>>();

    session
        .set_inputs(
            r#gen.name.as_str(),
            values
                .into_iter()
                .filter_map(|(k, v)| {
                    if skip.contains(k.as_str()) {
                        None
                    } else {
                        Some((k, v))
                    }
                })
                .collect(),
        )
        .await?;

    Ok(session)
}

fn expand_vars(
    key: &str,
    values: &UnorderedMap<String, serde_json::Value>,
    context_values: &UnorderedMap<String, OwnedValueBag>,
) -> Result<UnorderedMap<String, OwnedValueBag>, Error> {
    let tera_ctx = get_tera_context(context_values);
    let mut result = unordered_map!();
    let parent_key = Some(key);

    for (key, value) in values.iter() {
        let value = expand_json_value(&tera_ctx, parent_key, key, value)?;
        result.insert(
            key.to_string(),
            ValueBag::capture_serde1(value.as_ref()).to_owned(),
        );
    }

    Ok(result)
}

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
    EnumDiscriminants,
)]
#[serde(tag = "action", rename_all = "kebab-case")]
#[strum_discriminants(name(ActionType))]
pub enum Action {
    CreateFile { path: PathBuf },
    ModifyFile { path: PathBuf },
    RemoveFile { path: PathBuf },
    CreateDir { path: PathBuf },
    RemoveDir { path: PathBuf },
    RemoveDirAll { path: PathBuf },
    Rename { from: PathBuf, to: PathBuf },
    Copy { from: PathBuf, to: PathBuf },
}

impl Action {
    pub async fn infer_from<'a, TSys: GeneratorSys>(
        action: &'a sys_impl::Action,
        sys: &'a TSys,
    ) -> eyre::Result<Option<Self>> {
        Ok(match action {
            sys_impl::Action::Write { path, .. } => {
                let exists = sys.fs_exists_no_err_async(path).await;
                if exists {
                    Some(Self::ModifyFile { path: path.clone() })
                } else {
                    Some(Self::CreateFile { path: path.clone() })
                }
            }
            sys_impl::Action::Append { path, .. } => {
                let exists = sys.fs_exists_no_err_async(path).await;
                if exists {
                    Some(Self::ModifyFile { path: path.clone() })
                } else {
                    Some(Self::CreateFile { path: path.clone() })
                }
            }
            sys_impl::Action::CreateDir { path, .. } => {
                Some(Self::CreateDir { path: path.clone() })
            }
            sys_impl::Action::RemoveFile { path } => {
                Some(Self::RemoveFile { path: path.clone() })
            }
            sys_impl::Action::RemoveDir { path } => {
                Some(Self::RemoveDir { path: path.clone() })
            }
            sys_impl::Action::RemoveDirAll { path } => {
                Some(Self::RemoveDirAll { path: path.clone() })
            }
            sys_impl::Action::Rename { from, to } => Some(Self::Rename {
                from: from.clone(),
                to: to.clone(),
            }),
            sys_impl::Action::Copy { from, to } => Some(Self::Copy {
                from: from.clone(),
                to: to.clone(),
            }),
            sys_impl::Action::SetCurrentDir { .. } => None,
        })
    }
}

#[cfg(test)]
mod nesting_tests {
    use std::borrow::Cow;

    use omni_generator_configurations::{
        ActionConfiguration, BaseActionConfiguration, GeneratorConfiguration,
        InputValuesConfiguration, JsRuntimeOption,
        RunGeneratorActionConfiguration, RunJavaScriptActionConfiguration,
    };
    use omni_input_provider::scripted::ScriptedInputProvider;
    use system_traits::impls::RealSys;

    use super::*;
    use crate::action_handlers::test_harness::MockJsScriptRunner;

    fn base() -> BaseActionConfiguration {
        BaseActionConfiguration {
            r#if: None,
            name: None,
            in_progress_message: None,
            success_message: None,
            error_message: None,
        }
    }

    fn run_js(script: &str) -> ActionConfiguration {
        ActionConfiguration::RunJavaScript {
            action: RunJavaScriptActionConfiguration {
                base: base(),
                data: Default::default(),
                runtime: JsRuntimeOption::Auto,
                script: script.into(),
                capabilities: Default::default(),
            },
        }
    }

    fn run_gen(target: &str) -> ActionConfiguration {
        ActionConfiguration::RunGenerator {
            action: RunGeneratorActionConfiguration {
                base: base(),
                generator: target.to_string(),
                input_values: InputValuesConfiguration::default(),
                args: UnorderedMap::default(),
                output_dir: None,
                targets: UnorderedMap::default(),
            },
        }
    }

    fn caps(json: &str) -> CapabilityRules<Generator> {
        serde_json::from_str(json).expect("valid capability chain")
    }

    fn generator(
        name: &str,
        capabilities: CapabilityRules<Generator>,
        actions: Vec<ActionConfiguration>,
    ) -> GeneratorConfiguration {
        GeneratorConfiguration {
            // The generator dir (config_path parent) is where its scripts
            // resolve, so distinct dirs keep the two `.mjs` paths
            // distinguishable in the recorded invocations.
            config_path: PathBuf::from(format!(
                "/fake/{name}/generator.omni.yaml"
            )),
            scope_id: None,
            user_invocable: true,
            name: name.to_string(),
            display_name: None,
            description: None,
            inputs: vec![],
            actions,
            vars: UnorderedMap::default(),
            targets: UnorderedMap::default(),
            capabilities: omni_capabilities::CapabilityPolicyConfig::from_rules(
                capabilities,
            ),
        }
    }

    /// Runs `entry` (with `generators` available and `workspace_caps` as the
    /// floor) and returns, keyed by each executed script's file name, the
    /// JSON-serialized policy level stack it ran under and the number of levels
    /// in that stack. Centralizes the config/runner boilerplate shared by the
    /// nesting tests.
    async fn run_and_collect_stacks(
        entry: &GeneratorConfiguration,
        generators: &[Cow<'_, GeneratorConfiguration>],
        workspace_caps: &CapabilityRules<Generator>,
    ) -> std::collections::HashMap<String, (String, usize)> {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ws = tmp.path().to_path_buf();
        let empty_targets = UnorderedMap::default();
        let empty_values = UnorderedMap::default();
        let empty_ctx = UnorderedMap::default();
        let env = Map::default();
        let provider =
            ScriptedInputProvider::new(std::iter::empty::<(&str, &str)>());

        let config = RunConfig::builder()
            .output_dir(&ws)
            .workspace_dir(&ws)
            .current_dir(&ws)
            .target_overrides(&empty_targets)
            .input_values(&empty_values)
            .context_values(&empty_ctx)
            .env(&env)
            .available_generators(generators)
            .workspace_capabilities(workspace_caps)
            .input_provider(&provider)
            .subscriber(&NoopSubscriber)
            .build();

        let mock = MockJsScriptRunner::default();
        let sys = TransactionSys::new(RealSys);
        run_internal(
            entry,
            &config,
            &sys,
            &mock,
            0,
            std::slice::from_ref(workspace_caps),
            CapabilitiesStrictness::default(),
        )
        .await
        .expect("nested run should succeed");

        // Each recorded level stack is parallel to its invocation; correlate
        // them by the script's file name and serialize the stack so tests can
        // assert which patterns (levels) it carried, and how many.
        let invs = mock.invocations.lock().unwrap();
        let level_stacks = mock.levels.lock().unwrap();
        assert_eq!(level_stacks.len(), invs.len());
        invs.iter()
            .enumerate()
            .map(|(idx, (_rt, scripts))| {
                let name = std::path::Path::new(&scripts[0].path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                let stack = &level_stacks[idx];
                (name, (serde_json::to_string(stack).unwrap(), stack.len()))
            })
            .collect()
    }

    /// Under the shrink-only (attenuation) model a nested `run-generator`
    /// **inherits the calling generator's ceiling** and can only narrow it. The
    /// child's level stack must therefore be `workspace ⧺ parent-own ⧺
    /// child-own` (outermost → innermost), proving both that the parent's policy
    /// now binds the child and that the child still contributes its own level.
    /// The parent, by contrast, runs under `workspace ⧺ parent-own` only.
    #[tokio::test]
    async fn nested_generator_inherits_the_parents_ceiling() {
        // Three disjoint, easily-recognizable read patterns, one per level.
        let workspace_caps = caps(
            r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/shared/**"] }]"#,
        );
        let parent_caps = caps(
            r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/parent-only/**"] }]"#,
        );
        let child_caps = caps(
            r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/child-only/**"] }]"#,
        );

        let parent = generator(
            "parent",
            parent_caps,
            vec![run_js("parent.mjs"), run_gen("child")],
        );
        let child = generator("child", child_caps, vec![run_js("child.mjs")]);

        let generators = vec![Cow::Owned(parent.clone()), Cow::Owned(child)];
        let stacks =
            run_and_collect_stacks(&parent, &generators, &workspace_caps).await;

        // Two `run-javascript` dispatches: parent.mjs then child.mjs.
        assert_eq!(
            stacks.len(),
            2,
            "expected the parent and the nested script"
        );
        let (parent_json, parent_len) =
            stacks.get("parent.mjs").expect("parent.mjs must have run");
        let (child_json, child_len) =
            stacks.get("child.mjs").expect("child.mjs must have run");

        // The parent's stack = workspace floor ⧺ its own policy ⧺ the action's
        // (empty) policy. The child's rule must not leak up into the parent.
        assert!(parent_json.contains("@workspace/shared/**"));
        assert!(parent_json.contains("@workspace/parent-only/**"));
        assert!(
            !parent_json.contains("@workspace/child-only/**"),
            "the child's policy must not leak up into the parent"
        );

        // The child inherits the parent's whole ceiling AND adds its own level:
        // workspace ⧺ parent-only ⧺ child-only all appear. Crucially the
        // parent-only rule IS now present (propagated as an inherited ceiling),
        // which is the shrink-only behaviour.
        assert!(
            child_json.contains("@workspace/shared/**"),
            "workspace floor must be forwarded to the nested generator"
        );
        assert!(
            child_json.contains("@workspace/parent-only/**"),
            "the calling generator's own policy must bind the nested child"
        );
        assert!(child_json.contains("@workspace/child-only/**"));

        // The child carries strictly more levels than the parent (it inherits
        // the parent's own policy as an extra ceiling level), so distinct
        // policies still yield distinct pool fingerprints and no process reuse.
        assert!(
            child_len > parent_len,
            "the child must inherit an extra ceiling level from the parent"
        );
        assert_ne!(parent_json, child_json);
    }

    /// Deep-nesting regression: with three generator levels
    /// (grandparent → parent → child) the ceiling must **accumulate** at every
    /// `run-generator` boundary, not collapse to the immediate parent. The
    /// grandparent's own policy must still bind the leaf child even though an
    /// intervening parent level sits between them — proving the chain is not
    /// flattened to a single parent step.
    #[tokio::test]
    async fn deeply_nested_generator_inherits_the_whole_ancestor_chain() {
        // One disjoint, easily-recognizable read pattern per level.
        let workspace_caps = caps(
            r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/shared/**"] }]"#,
        );
        let grandparent_caps = caps(
            r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/grandparent-only/**"] }]"#,
        );
        let parent_caps = caps(
            r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/parent-only/**"] }]"#,
        );
        let child_caps = caps(
            r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/child-only/**"] }]"#,
        );

        let grandparent = generator(
            "grandparent",
            grandparent_caps,
            vec![run_js("grandparent.mjs"), run_gen("parent")],
        );
        let parent = generator(
            "parent",
            parent_caps,
            vec![run_js("parent.mjs"), run_gen("child")],
        );
        let child = generator("child", child_caps, vec![run_js("child.mjs")]);

        let generators = vec![
            Cow::Owned(grandparent.clone()),
            Cow::Owned(parent),
            Cow::Owned(child),
        ];
        let stacks =
            run_and_collect_stacks(&grandparent, &generators, &workspace_caps)
                .await;

        // Three `run-javascript` dispatches: grandparent.mjs, parent.mjs,
        // child.mjs.
        assert_eq!(stacks.len(), 3, "expected all three nested scripts");
        let (grandparent_json, _) = stacks
            .get("grandparent.mjs")
            .expect("grandparent.mjs must have run");
        let (parent_json, _) =
            stacks.get("parent.mjs").expect("parent.mjs must have run");
        let (child_json, _) =
            stacks.get("child.mjs").expect("child.mjs must have run");

        // Grandparent runs under workspace ⧺ its own policy only; no descendant
        // rule may leak upward.
        assert!(grandparent_json.contains("@workspace/shared/**"));
        assert!(grandparent_json.contains("@workspace/grandparent-only/**"));
        assert!(
            !grandparent_json.contains("@workspace/parent-only/**"),
            "a descendant's policy must not leak up into the grandparent"
        );
        assert!(!grandparent_json.contains("@workspace/child-only/**"));

        // Parent inherits workspace ⧺ grandparent, adds its own; the child's
        // rule must still not leak up.
        assert!(parent_json.contains("@workspace/shared/**"));
        assert!(
            parent_json.contains("@workspace/grandparent-only/**"),
            "the grandparent's policy must bind the parent"
        );
        assert!(parent_json.contains("@workspace/parent-only/**"));
        assert!(
            !parent_json.contains("@workspace/child-only/**"),
            "the child's policy must not leak up into the parent"
        );

        // The leaf child inherits the WHOLE ancestor chain: workspace ⧺
        // grandparent ⧺ parent, plus its own level. The critical assertion is
        // that the grandparent-only rule — two levels up, across an intervening
        // parent — still appears, i.e. the ceiling accumulates rather than
        // collapsing to the immediate parent.
        assert!(
            child_json.contains("@workspace/shared/**"),
            "workspace floor must reach the leaf child"
        );
        assert!(
            child_json.contains("@workspace/grandparent-only/**"),
            "the grandparent's policy must bind the leaf child across the intervening parent level"
        );
        assert!(
            child_json.contains("@workspace/parent-only/**"),
            "the immediate parent's policy must bind the leaf child"
        );
        assert!(child_json.contains("@workspace/child-only/**"));
    }

    /// An action whose own policy declares a `require-floor` stance.
    fn run_js_strict(script: &str) -> ActionConfiguration {
        ActionConfiguration::RunJavaScript {
            action: RunJavaScriptActionConfiguration {
                base: base(),
                data: Default::default(),
                runtime: JsRuntimeOption::Auto,
                script: script.into(),
                capabilities: omni_capabilities::CapabilityPolicyConfig {
                    rules: Default::default(),
                    strictness: CapabilitiesStrictness::RequireFloor,
                },
            },
        }
    }

    /// A generator whose own policy declares the given strictness stance.
    fn generator_with_strictness(
        name: &str,
        strictness: CapabilitiesStrictness,
        actions: Vec<ActionConfiguration>,
    ) -> GeneratorConfiguration {
        let mut g = generator(name, CapabilityRules::default(), actions);
        g.capabilities.strictness = strictness;
        g
    }

    /// Runs `entry` seeding the accumulated stance with `workspace_strictness`
    /// and returns, keyed by each executed script's file name, the effective
    /// [`CapabilitiesStrictness`] the script actually ran under.
    async fn run_and_collect_strictness(
        entry: &GeneratorConfiguration,
        generators: &[Cow<'_, GeneratorConfiguration>],
        workspace_strictness: CapabilitiesStrictness,
    ) -> std::collections::HashMap<String, CapabilitiesStrictness> {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ws = tmp.path().to_path_buf();
        let empty_targets = UnorderedMap::default();
        let empty_values = UnorderedMap::default();
        let empty_ctx = UnorderedMap::default();
        let env = Map::default();
        let workspace_caps = CapabilityRules::<Generator>::default();
        let provider =
            ScriptedInputProvider::new(std::iter::empty::<(&str, &str)>());

        let config = RunConfig::builder()
            .output_dir(&ws)
            .workspace_dir(&ws)
            .current_dir(&ws)
            .target_overrides(&empty_targets)
            .input_values(&empty_values)
            .context_values(&empty_ctx)
            .env(&env)
            .available_generators(generators)
            .workspace_capabilities(&workspace_caps)
            .workspace_strictness(workspace_strictness)
            .input_provider(&provider)
            .subscriber(&NoopSubscriber)
            .build();

        let mock = MockJsScriptRunner::default();
        let sys = TransactionSys::new(RealSys);
        run_internal(
            entry,
            &config,
            &sys,
            &mock,
            0,
            std::slice::from_ref(&workspace_caps),
            workspace_strictness,
        )
        .await
        .expect("run should succeed");

        let invs = mock.invocations.lock().unwrap();
        let strictness = mock.strictness.lock().unwrap();
        assert_eq!(strictness.len(), invs.len());
        invs.iter()
            .enumerate()
            .map(|(idx, (_rt, scripts))| {
                let name = std::path::Path::new(&scripts[0].path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                (name, strictness[idx])
            })
            .collect()
    }

    /// A `require-floor` workspace stance must reach a `warn` generator's script
    /// (strictness combines most-severe, it does not attenuate like rules).
    #[tokio::test]
    async fn workspace_require_floor_binds_a_warn_generator() {
        let g = generator_with_strictness(
            "g",
            CapabilitiesStrictness::Warn,
            vec![run_js("g.mjs")],
        );
        let generators = vec![Cow::Owned(g.clone())];
        let out = run_and_collect_strictness(
            &g,
            &generators,
            CapabilitiesStrictness::RequireFloor,
        )
        .await;
        assert_eq!(out["g.mjs"], CapabilitiesStrictness::RequireFloor);
    }

    /// A generator's own `require-floor` stance tightens a `warn` workspace.
    #[tokio::test]
    async fn generator_require_floor_tightens_a_warn_workspace() {
        let g = generator_with_strictness(
            "g",
            CapabilitiesStrictness::RequireFloor,
            vec![run_js("g.mjs")],
        );
        let generators = vec![Cow::Owned(g.clone())];
        let out = run_and_collect_strictness(
            &g,
            &generators,
            CapabilitiesStrictness::Warn,
        )
        .await;
        assert_eq!(out["g.mjs"], CapabilitiesStrictness::RequireFloor);
    }

    /// An action's own `require-floor` stance tightens a `warn` generator.
    #[tokio::test]
    async fn action_require_floor_tightens_a_warn_generator() {
        let g = generator_with_strictness(
            "g",
            CapabilitiesStrictness::Warn,
            vec![run_js_strict("g.mjs")],
        );
        let generators = vec![Cow::Owned(g.clone())];
        let out = run_and_collect_strictness(
            &g,
            &generators,
            CapabilitiesStrictness::Warn,
        )
        .await;
        assert_eq!(out["g.mjs"], CapabilitiesStrictness::RequireFloor);
    }

    /// Strictness accumulates most-severe across a deep chain: a `require-floor`
    /// grandparent binds a leaf child two levels down, even though the
    /// intervening parent and the child are both `warn`.
    #[tokio::test]
    async fn strictness_accumulates_across_the_ancestor_chain() {
        let grandparent = generator_with_strictness(
            "grandparent",
            CapabilitiesStrictness::RequireFloor,
            vec![run_js("grandparent.mjs"), run_gen("parent")],
        );
        let parent = generator_with_strictness(
            "parent",
            CapabilitiesStrictness::Warn,
            vec![run_js("parent.mjs"), run_gen("child")],
        );
        let child = generator_with_strictness(
            "child",
            CapabilitiesStrictness::Warn,
            vec![run_js("child.mjs")],
        );
        let generators = vec![
            Cow::Owned(grandparent.clone()),
            Cow::Owned(parent),
            Cow::Owned(child),
        ];
        let out = run_and_collect_strictness(
            &grandparent,
            &generators,
            CapabilitiesStrictness::Warn,
        )
        .await;
        // Every script from the require-floor grandparent downward inherits it.
        assert_eq!(
            out["grandparent.mjs"],
            CapabilitiesStrictness::RequireFloor
        );
        assert_eq!(out["parent.mjs"], CapabilitiesStrictness::RequireFloor);
        assert_eq!(out["child.mjs"], CapabilitiesStrictness::RequireFloor);
    }
}
