use std::{borrow::Cow, path::PathBuf};

use js_runtime::impls::DelegatingJsRuntimeOption;
use maps::{Map, UnorderedMap};
use omni_generator_configurations::{
    GeneratorConfiguration, OmniPath, OverwriteConfiguration, Root,
};
use omni_input_provider::scripted::ScriptedInputProvider;
use system_traits::impls::RealSys;
use tempfile::TempDir;
use value_bag::{OwnedValueBag, ValueBag};

use crate::{
    GenSession, JsScriptRunner, LazyScriptRunner, ScriptInvocation,
    TransactionSys, action_handlers::HandlerContext,
};

/// Records every `run_scripts` call for test assertions.
/// Uses an `Arc<Mutex>` so the mock can be cloned before being boxed into the fixture.
#[derive(Debug, Default, Clone)]
pub struct MockJsScriptRunner {
    pub invocations: std::sync::Arc<
        std::sync::Mutex<
            Vec<(DelegatingJsRuntimeOption, Vec<ScriptInvocation>)>,
        >,
    >,
}

#[async_trait::async_trait]
impl JsScriptRunner for MockJsScriptRunner {
    async fn run_scripts(
        &self,
        runtime: DelegatingJsRuntimeOption,
        invocations: &[ScriptInvocation],
    ) -> Result<(), crate::error::Error> {
        self.invocations
            .lock()
            .unwrap()
            .push((runtime, invocations.to_vec()));
        Ok(())
    }
}

/// Shared test fixture that owns every value referenced by [`HandlerContext`].
/// Use the builder methods to configure values and targets before calling [`ctx`][Fixture::ctx].
pub struct Fixture {
    pub output: TempDir,
    pub generator: TempDir,
    workspace: PathBuf,
    pub context_values: UnorderedMap<String, OwnedValueBag>,
    pub tera_ctx: omni_tera::Context,
    pub generator_targets: UnorderedMap<String, OmniPath>,
    target_overrides: UnorderedMap<String, OmniPath>,
    gen_session: GenSession,
    js_script_runner: Box<dyn JsScriptRunner>,
    input_provider: ScriptedInputProvider,
    pub env: Map<String, String>,
    pub overwrite: Option<OverwriteConfiguration>,
    generators: Vec<Cow<'static, GeneratorConfiguration>>,
}

impl Fixture {
    pub fn new() -> Self {
        let output = TempDir::new().expect("output TempDir");
        let generator = TempDir::new().expect("generator TempDir");
        let workspace = output.path().to_path_buf();
        let js_script_runner: Box<dyn JsScriptRunner> =
            Box::new(LazyScriptRunner::new(
                TransactionSys::new(RealSys),
                std::env::temp_dir(),
                "0.0.0-test".to_string(),
            ));
        Self {
            workspace,
            output,
            generator,
            context_values: UnorderedMap::default(),
            tera_ctx: omni_tera::Context::new(),
            generator_targets: UnorderedMap::default(),
            target_overrides: UnorderedMap::default(),
            gen_session: GenSession::new(),
            js_script_runner,
            input_provider: ScriptedInputProvider::new(std::iter::empty::<(
                &str,
                &str,
            )>()),
            env: Map::default(),
            overwrite: None,
            generators: vec![],
        }
    }

    /// Inserts a key/value into both the context map and the Tera context.
    pub fn with_value(
        mut self,
        key: &str,
        value: impl serde::Serialize + 'static,
    ) -> Self {
        self.context_values.insert(
            key.to_string(),
            ValueBag::capture_serde1(&value).to_owned(),
        );
        self.tera_ctx.insert(key, &value);
        self
    }

    /// Maps `name` to `output_dir / rel_path` in `generator_targets`.
    pub fn with_output_target(mut self, name: &str, rel_path: &str) -> Self {
        self.generator_targets.insert(
            name.to_string(),
            OmniPath::new_rooted(rel_path, Root::Output),
        );
        self
    }

    pub fn with_overwrite(mut self, ow: OverwriteConfiguration) -> Self {
        self.overwrite = Some(ow);
        self
    }

    pub fn with_js_script_runner(
        mut self,
        runner: Box<dyn JsScriptRunner>,
    ) -> Self {
        self.js_script_runner = runner;
        self
    }

    pub fn ctx(&self) -> HandlerContext<'_> {
        HandlerContext {
            dry_run: false,
            output_dir: self.output.path(),
            generator_dir: self.generator.path(),
            generator_name: "test_generator",
            scope_id: None,
            current_dir: self.output.path(),
            context_values: &self.context_values,
            tera_context_values: &self.tera_ctx,
            generator_targets: &self.generator_targets,
            target_overrides: &self.target_overrides,
            overwrite: self.overwrite,
            available_generators: &self.generators,
            workspace_dir: &self.workspace,
            resolved_action_name: "test_action",
            env: &self.env,
            gen_session: &self.gen_session,
            js_script_runner: self.js_script_runner.as_ref(),
            input_provider: &self.input_provider,
        }
    }
}
