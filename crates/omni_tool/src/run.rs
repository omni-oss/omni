use std::{borrow::Cow, future::Future, path::Path, pin::Pin};

use async_trait::async_trait;
use bridge_rpc_runner::{DEFAULT_MAX_DEPTH, EffectivePolicy};
use omni_capabilities::{
    CapabilitiesStrictness, CapabilityRules, PathRoots, Root,
};
use omni_tool_configurations::{
    Tool, ToolBackend, ToolConfiguration, ToolJsRuntime,
};
use serde::Serialize;
use serde_json::Value;

use crate::error::{Error, ErrorInner};

/// Path of the `exec-tool` service exposed by the bridge service.
pub const EXEC_TOOL_PATH: &str = "/exec-tool";

/// The workspace-level capability inputs a tool run is confined under, combined
/// with each tool's own declared policy to form its effective policy.
///
/// Mirrors the generator's workspace floor / roots / strictness / enforce
/// wiring, specialized to the [`Tool`] profile (tools have no ancestors, so the
/// only inherited level is the workspace floor).
pub struct ToolEnforcement {
    /// The workspace-level floor for the tool subsystem, folded ahead of — and
    /// unwidenable by — each tool's own policy.
    pub workspace_floor: CapabilityRules<Tool>,
    /// Path roots used to resolve `@workspace/…`-style patterns.
    pub roots: PathRoots<Root>,
    /// Workspace-level floor-gap stance, combined most-severe with each tool's.
    pub workspace_strictness: CapabilitiesStrictness,
    /// Whether to enforce (the experimental `capabilities` feature is on).
    pub enforce: bool,
}

/// The `{ path, cwd, inputs }` payload sent to the `/exec-tool` service.
#[derive(Debug, Serialize)]
pub struct ExecToolPayload<'a> {
    /// Absolute path to the tool's JavaScript entrypoint.
    pub path: &'a str,
    /// Base directory relative paths resolve against (the workspace root).
    pub cwd: &'a str,
    /// The tool's already-validated inputs.
    pub inputs: &'a Value,
}

/// Abstraction over the JavaScript execution backend for a single tool.
///
/// Implemented by the real bridge-backed runner and by test doubles.
#[async_trait]
pub trait ToolRunner: Send + Sync {
    /// Execute the tool entrypoint at `entrypoint` on `runtime` with the given
    /// validated `inputs`, confined under `policy`, returning the value the
    /// tool's default export returned.
    async fn run_js(
        &self,
        entrypoint: &Path,
        runtime: ToolJsRuntime,
        inputs: &Value,
        policy: &EffectivePolicy<Tool>,
    ) -> Result<Value, Error>;
}

/// Resolve and run the tool named `name` from `tools`, returning its JSON
/// return value.
///
/// The tool's `entrypoint` is resolved relative to the directory containing its
/// manifest. Its effective policy is the workspace floor folded ahead of the
/// tool's own declared capabilities. A `type: pipeline` tool chains other tools
/// (including other pipelines), bounded by [`DEFAULT_MAX_DEPTH`].
pub async fn run_named<R: ToolRunner>(
    tools: &[Cow<'_, ToolConfiguration>],
    name: &str,
    inputs: Value,
    runner: &R,
    enforcement: &ToolEnforcement,
) -> Result<Value, Error> {
    run_named_at_depth(tools, name, inputs, runner, enforcement, 0).await
}

/// Depth-tracked core of [`run_named`]. Boxed so a `type: pipeline` tool can
/// recursively invoke other tools without an infinitely-sized future.
pub(crate) fn run_named_at_depth<'a, R: ToolRunner>(
    tools: &'a [Cow<'a, ToolConfiguration>],
    name: &'a str,
    inputs: Value,
    runner: &'a R,
    enforcement: &'a ToolEnforcement,
    depth: usize,
) -> Pin<Box<dyn Future<Output = Result<Value, Error>> + Send + 'a>> {
    Box::pin(async move {
        // Defense-in-depth backstop against a cycle between pipeline tools.
        if depth > DEFAULT_MAX_DEPTH {
            return Err(Error::from(ErrorInner::MaxDepthExceeded {
                max: DEFAULT_MAX_DEPTH,
            }));
        }

        let tool = tools.iter().find(|t| t.name == name).ok_or_else(|| {
            Error::from(ErrorInner::ToolNotFound {
                name: name.to_string(),
            })
        })?;

        match &tool.backend {
            ToolBackend::Js {
                runtime,
                entrypoint,
            } => {
                let base =
                    tool.config_path.parent().unwrap_or_else(|| Path::new("."));
                let resolved = base.join(entrypoint);

                // The effective policy is a stack of **distinct** levels
                // ordered outermost → innermost: the workspace floor, then the
                // tool's own policy. Authorization applies the shrink-only
                // (attenuation) model so the tool can only narrow the workspace
                // floor, never widen it.
                let levels = vec![
                    enforcement.workspace_floor.clone(),
                    tool.capabilities.rules.clone(),
                ];
                let policy = EffectivePolicy {
                    levels,
                    roots: enforcement.roots.clone(),
                    context: (),
                    strictness: enforcement
                        .workspace_strictness
                        .max(tool.capabilities.strictness),
                    enforce: enforcement.enforce,
                };

                runner.run_js(&resolved, *runtime, &inputs, &policy).await
            }
            ToolBackend::Pipeline { steps, output } => {
                crate::pipeline::run_pipeline(
                    tools,
                    steps,
                    *output,
                    inputs,
                    runner,
                    enforcement,
                    depth,
                )
                .await
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use omni_tool_configurations::ToolBackend;
    use serde_json::json;

    use super::*;

    #[derive(Default)]
    struct MockRunner {
        last_path: std::sync::Mutex<Option<PathBuf>>,
        last_inputs: std::sync::Mutex<Option<Value>>,
    }

    #[async_trait]
    impl ToolRunner for MockRunner {
        async fn run_js(
            &self,
            entrypoint: &Path,
            _runtime: ToolJsRuntime,
            inputs: &Value,
            _policy: &EffectivePolicy<Tool>,
        ) -> Result<Value, Error> {
            *self.last_path.lock().unwrap() = Some(entrypoint.to_path_buf());
            *self.last_inputs.lock().unwrap() = Some(inputs.clone());
            Ok(json!({ "ran": entrypoint.to_string_lossy(), "inputs": inputs }))
        }
    }

    fn no_enforcement() -> ToolEnforcement {
        ToolEnforcement {
            workspace_floor: CapabilityRules::default(),
            roots: PathRoots::new(),
            workspace_strictness: CapabilitiesStrictness::Warn,
            enforce: false,
        }
    }

    fn js_tool(
        name: &str,
        config_path: &str,
        entrypoint: &str,
    ) -> ToolConfiguration {
        ToolConfiguration {
            config_path: PathBuf::from(config_path),
            name: name.to_string(),
            description: None,
            inputs: vec![],
            capabilities: Default::default(),
            backend: ToolBackend::Js {
                runtime: ToolJsRuntime::Deno,
                entrypoint: entrypoint.to_string(),
            },
        }
    }

    #[tokio::test]
    async fn run_named_resolves_entrypoint_relative_to_manifest() {
        let tools = vec![Cow::Owned(js_tool(
            "greet",
            "/ws/tools/greet/tool.omni.yaml",
            "./index.mjs",
        ))];
        let runner = MockRunner::default();

        let result = run_named(
            &tools,
            "greet",
            json!({ "who": "world" }),
            &runner,
            &no_enforcement(),
        )
        .await
        .expect("run succeeds");

        let path = runner.last_path.lock().unwrap().clone().unwrap();
        assert_eq!(path, PathBuf::from("/ws/tools/greet/index.mjs"));
        assert_eq!(
            runner.last_inputs.lock().unwrap().clone().unwrap(),
            json!({ "who": "world" })
        );
        assert_eq!(result["inputs"], json!({ "who": "world" }));
    }

    #[tokio::test]
    async fn run_named_errors_for_unknown_tool() {
        let tools: Vec<Cow<'_, ToolConfiguration>> = vec![];
        let runner = MockRunner::default();

        let err =
            run_named(&tools, "missing", json!({}), &runner, &no_enforcement())
                .await
                .expect_err("unknown tool errors");
        assert!(matches!(err.kind(), crate::error::ErrorKind::ToolNotFound));
    }

    #[tokio::test]
    async fn run_named_dispatches_pipeline_steps() {
        // A pipeline routes a `from: inputs.*` reference into a js step's
        // inputs and returns the last step's output.
        let tools = vec![
            Cow::Owned(ToolConfiguration {
                config_path: PathBuf::from("/ws/tools/p/tool.omni.yaml"),
                name: "p".to_string(),
                description: None,
                inputs: vec![],
                capabilities: Default::default(),
                backend: ToolBackend::Pipeline {
                    steps: vec![omni_tool_configurations::PipelineStep {
                        name: "greeting".to_string(),
                        tool: "greet".to_string(),
                        inputs: [(
                            "who".to_string(),
                            omni_tool_configurations::PipelineInputValue::Ref {
                                from: "inputs.who".to_string(),
                            },
                        )]
                        .into_iter()
                        .collect(),
                        r#if: None,
                    }],
                    output: Default::default(),
                },
            }),
            Cow::Owned(js_tool(
                "greet",
                "/ws/tools/greet/tool.omni.yaml",
                "./index.mjs",
            )),
        ];
        let runner = MockRunner::default();

        let result = run_named(
            &tools,
            "p",
            json!({ "who": "world" }),
            &runner,
            &no_enforcement(),
        )
        .await
        .expect("pipeline runs");

        // The js step received the routed input.
        assert_eq!(
            runner.last_inputs.lock().unwrap().clone().unwrap(),
            json!({ "who": "world" })
        );
        // The pipeline returns the last step's output (the mock's echo).
        assert_eq!(result["inputs"], json!({ "who": "world" }));
    }

    #[tokio::test]
    async fn run_named_empty_pipeline_returns_null() {
        let tools = vec![Cow::Owned(ToolConfiguration {
            config_path: PathBuf::from("/ws/tools/p/tool.omni.yaml"),
            name: "p".to_string(),
            description: None,
            inputs: vec![],
            capabilities: Default::default(),
            backend: ToolBackend::Pipeline {
                steps: vec![],
                output: Default::default(),
            },
        })];
        let runner = MockRunner::default();

        let result =
            run_named(&tools, "p", json!({}), &runner, &no_enforcement())
                .await
                .expect("empty pipeline runs");
        assert_eq!(result, Value::Null);
    }
}
