use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use async_trait::async_trait;
use bridge_rpc_router::Router;
use bridge_rpc_runner::{
    BridgeRunnerOptions, BridgeServiceRunner, DelegatingJsRuntimeOption,
    EffectivePolicy, RunnerPool, SpawnPosture, VendoredBridgeService,
    build_spawn_plan, build_unconfined_plan, resolved_exec_path,
};
use bridge_rpc_services::{
    FsSys, ProcSys, RegisterServicesOptions, register_services_with_defaults,
};
use omni_capabilities::CapabilityFloors;
use omni_capability_enforcement::ShimPolicy;
use omni_capability_sys::{EvaluatingAuthorizer, PolicyEnforcingSys};
use omni_tool_configurations::{Tool, ToolJsRuntime};
use serde_json::Value;
use system_traits::{BaseFsMetadataAsync, EnvSnapshot, EnvVars};

use crate::{
    error::{Error, ErrorInner},
    run::{EXEC_TOOL_PATH, ExecToolPayload, ToolRunner},
};

/// A lazily-spawned, bridge-backed [`ToolRunner`].
///
/// The backing JS process is the vendored bridge-service, spawned on first use
/// and pooled per `(resolved runtime, effective-policy fingerprint)`. Tools
/// with the same runtime and identical effective policy reuse one process; a
/// tool with a different policy gets its own process launched under its own
/// pre-spawn [`SpawnPolicy`] and its own in-process broker.
///
/// Enforcement follows the experimental `capabilities` feature (carried on
/// [`EffectivePolicy::enforce`]): when enabled, the tool's cascaded policy is
/// planned and confined (a tool that declares nothing is held to the built-in
/// floor); when disabled, the tool runs unconfined (Deno launches with
/// `--allow-all` and a pass-through broker).
pub struct LazyToolRunner<S> {
    sys: S,
    context_dir: PathBuf,
    /// Base directory the tool's relative `ctx.sys` paths resolve against.
    /// Governs the capability-enforced surface via the in-runtime cwd system;
    /// defaults to the workspace root when the caller supplies no working dir.
    working_dir: PathBuf,
    version: String,
    pool: RunnerPool<(DelegatingJsRuntimeOption, String)>,
}

impl<S> LazyToolRunner<S> {
    pub fn new(
        sys: S,
        context_dir: PathBuf,
        working_dir: PathBuf,
        version: String,
    ) -> Self {
        Self {
            sys,
            context_dir,
            working_dir,
            version,
            pool: RunnerPool::new(),
        }
    }

    /// Shuts down every spawned runner. Best-effort.
    pub async fn shutdown(&self) {
        self.pool.shutdown().await;
    }
}

fn map_runtime(runtime: ToolJsRuntime) -> DelegatingJsRuntimeOption {
    match runtime {
        ToolJsRuntime::Deno => DelegatingJsRuntimeOption::Deno,
        ToolJsRuntime::Node => DelegatingJsRuntimeOption::Node,
        ToolJsRuntime::Bun => DelegatingJsRuntimeOption::Bun,
        ToolJsRuntime::Auto => DelegatingJsRuntimeOption::Auto,
    }
}

fn runtime_name(runtime: DelegatingJsRuntimeOption) -> &'static str {
    match runtime {
        DelegatingJsRuntimeOption::Deno => "deno",
        DelegatingJsRuntimeOption::Node => "node",
        DelegatingJsRuntimeOption::Bun => "bun",
        DelegatingJsRuntimeOption::Auto => "auto",
    }
}

#[async_trait]
impl<S> ToolRunner for LazyToolRunner<S>
where
    S: FsSys + ProcSys + EnvVars + Clone + Send + Sync + 'static,
    <S as BaseFsMetadataAsync>::Metadata: Send,
{
    async fn run_js(
        &self,
        entrypoint: &Path,
        runtime: ToolJsRuntime,
        inputs: &Value,
        policy: &EffectivePolicy<Tool>,
    ) -> Result<Value, Error> {
        // `Auto` resolves against PATH; a concrete runtime is returned as-is.
        let resolved = map_runtime(runtime)
            .resolve()
            .ok_or(ErrorInner::NoJsRuntime)?;

        // A selected (or auto-resolved) runtime that is not actually on PATH is
        // a clear, actionable error rather than a mid-handshake spawn failure.
        if resolved_exec_path(resolved).is_none() {
            return Err(ErrorInner::RuntimeNotFound {
                runtime: runtime_name(resolved).to_string(),
            }
            .into());
        }

        // Only Deno is fully confined on every platform; node/bun are
        // experimental (missing or partial permission flags). Warn whenever the
        // launch would actually use one — either explicitly selected or because
        // `auto` resolved to it — so the weaker sandboxing is visible.
        match resolved {
            DelegatingJsRuntimeOption::Node
            | DelegatingJsRuntimeOption::Bun => {
                let how = if matches!(runtime, ToolJsRuntime::Auto) {
                    "auto-detection resolved to"
                } else {
                    "you selected"
                };
                trace::warn!(
                    "{how} the `{}` runtime, which is experimental and not \
                     fully sandboxed on every platform; only `deno` is fully \
                     supported. Install/select `deno` for enforced capability \
                     confinement.",
                    runtime_name(resolved)
                );
            }
            DelegatingJsRuntimeOption::Deno
            | DelegatingJsRuntimeOption::Auto => {}
        }

        // Enforcement is gated on the experimental capabilities feature. When
        // enabled, the declared policy is planned and enforced (a tool that
        // declares none is confined to the built-in floor). When disabled, the
        // tool runs unconfined: an allow-everything spawn plan and a
        // pass-through broker.
        let (spawn_policy, shim_policy) = if policy.enforce {
            let (spawn, shim, diagnostics) =
                build_spawn_plan(resolved, policy, SpawnPosture::from_env())
                    .map_err(|e| ErrorInner::CapabilityEnforcement {
                        message: e.to_string(),
                    })?;
            for diagnostic in diagnostics {
                trace::warn!("{}", diagnostic.message);
            }
            (spawn, shim)
        } else {
            if policy.levels.iter().any(|level| !level.is_empty()) {
                trace::warn!(
                    "a capability policy is declared but the capabilities \
                     feature is experimental and disabled; it is ignored and \
                     the tool runs unconfined — enable it with \
                     `enable_experimental: true` (or `enable_experimental: \
                     {{ capabilities: true }}`) in the workspace configuration"
                );
            }
            (build_unconfined_plan(resolved), ShimPolicy::new())
        };

        let shim_json = if shim_policy.is_empty() {
            String::new()
        } else {
            shim_policy.to_json()
        };

        // Canonicalize the root bases so the enforcing sys can re-authorize a
        // symlink-resolved *real* path without a root that itself lives under a
        // symlink being misread as an escape. A base that does not (yet) exist
        // is left as-is.
        let roots = policy
            .roots
            .clone()
            .map_bases(|base| std::fs::canonicalize(&base).unwrap_or(base));
        let authorizer = if policy.enforce {
            EvaluatingAuthorizer::layered(policy.effective_levels(), roots, ())
        } else {
            EvaluatingAuthorizer::layered(
                vec![Tool::unconfined_authorizer_chain()],
                roots,
                (),
            )
        };

        let sys = self.sys.clone();
        let context_dir = self.context_dir.clone();
        let version = self.version.clone();
        let key = (resolved, policy.fingerprint());

        let runner = self
            .pool
            .get_or_try_init(key, move || {
                Box::pin(async move {
                    let vendored =
                        VendoredBridgeService::new(version, None::<String>)
                            .ensure(&context_dir)
                            .await
                            .map_err(Error::from)?;

                    let mut spawn_policy = spawn_policy;

                    // On Windows the confined child is granted only a minimal
                    // boot set, so the vendored bundle root must be granted
                    // explicitly or the runtime cannot read its own entrypoint.
                    #[cfg(target_os = "windows")]
                    if let Some(spec) = spawn_policy.os_sandbox.as_mut() {
                        spec.read_paths.push(vendored.root.clone());
                    }

                    let mut router = Router::new();
                    // Broker every mediated fs operation against the tool's
                    // effective policy before it touches `sys`, and filter
                    // `env` by that same policy.
                    let enforcing = PolicyEnforcingSys::new(sys, authorizer);

                    // Scrub the child's ambient environment down to the
                    // policy-allowed snapshot so a script cannot read an
                    // un-mediated variable through `process.env` / `Deno.env`.
                    spawn_policy.env = Some(enforcing.env_snapshot());

                    register_services_with_defaults(
                        &mut router,
                        Arc::new(enforcing),
                        RegisterServicesOptions::default(),
                    );

                    // The bridge-service CLI expects a `run` subcommand. When
                    // the launch flags could not confine `net`/`process`
                    // precisely, the residual policy is handed to the in-runtime
                    // shim via `--enforce <json>`; an empty residual is omitted.
                    let mut script_args: Vec<&str> = vec!["run"];
                    if !shim_json.is_empty() {
                        script_args.push("--enforce");
                        script_args.push(&shim_json);
                    }

                    BridgeServiceRunner::spawn(
                        router,
                        BridgeRunnerOptions::new(
                            &vendored.entrypoint,
                            resolved,
                            &spawn_policy,
                        )
                        .with_cwd(Some(&context_dir))
                        .with_script_args(&script_args),
                    )
                    .await
                    .map_err(Error::from)
                })
            })
            .await?;

        let path = entrypoint.to_string_lossy();
        // The working dir is the base relative `ctx.sys` paths resolve against
        // (via the in-runtime cwd system). It is sent per call, so one pooled
        // process can serve invocations with different working dirs.
        let cwd = self.working_dir.to_string_lossy();
        let payload = ExecToolPayload {
            path: path.as_ref(),
            cwd: cwd.as_ref(),
            inputs,
        };

        // On Windows the confined child (AppContainer) is granted only a
        // minimal boot set at spawn — enough to load the vendored bundle, but
        // not the tool's own entrypoint. Grant read+execute to the entrypoint's
        // directory for the duration of the call (recursive, so a bundled
        // entrypoint's sibling chunks are covered too) so the runtime's module
        // loader can import it, then revoke on drop. A no-op off Windows and for
        // an unconfined child. The tool's own directory is within the
        // `@workspace/**` fs.read floor, so this grants no authority the policy
        // does not already permit.
        let read_scope = if runner.is_confined() {
            let dir = entrypoint
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| entrypoint.to_path_buf());
            runner.grant_read_scope(&[dir])
        } else {
            runner.grant_read_scope(&[])
        };

        let body = runner.call(EXEC_TOOL_PATH, &payload).await?;
        drop(read_scope);
        let value: Value = serde_json::from_slice(&body)?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use bridge_rpc_runner::{
        DelegatingJsRuntimeOption, EffectivePolicy, SpawnPosture,
        build_spawn_plan, build_unconfined_plan,
    };
    use omni_capabilities::{
        CapabilitiesStrictness, CapabilityRules, PathRoots, Root,
    };
    use omni_tool_configurations::Tool;

    fn policy(
        chain: CapabilityRules<Tool>,
        strictness: CapabilitiesStrictness,
        enforce: bool,
    ) -> EffectivePolicy<Tool> {
        EffectivePolicy {
            levels: vec![chain],
            roots: PathRoots::new().with(Root::Workspace, "/repo"),
            context: (),
            strictness,
            enforce,
        }
    }

    #[test]
    fn a_tool_declaring_nothing_falls_to_the_built_in_workspace_floor() {
        // Empty levels are dropped and the built-in floor stands in, so a
        // capability-free tool is confined to `@workspace/**`, never unconfined.
        let p = policy(
            CapabilityRules::default(),
            CapabilitiesStrictness::Warn,
            true,
        );
        let levels = p.effective_levels();
        assert_eq!(levels.len(), 1);
        let json = serde_json::to_value(&levels[0]).unwrap();
        let patterns = json.to_string();
        assert!(patterns.contains("@workspace/**"), "{patterns}");
        assert!(patterns.contains("fs.write"), "{patterns}");
    }

    #[test]
    fn require_floor_refuses_net_on_bun() {
        // `net` is governed but has no un-bypassable floor on Bun, so
        // `require-floor` must refuse rather than run under a bypassable shim.
        let chain: CapabilityRules<Tool> = serde_json::from_str(
            r#"[{ "access": "allow", "domain": "net", "patterns": ["example.com:443"] }]"#,
        )
        .expect("valid net chain");
        let p = policy(chain, CapabilitiesStrictness::RequireFloor, true);
        let err = build_spawn_plan(
            DelegatingJsRuntimeOption::Bun,
            &p,
            SpawnPosture::default(),
        )
        .expect_err("require-floor refuses an unfloored governed domain");
        assert!(!format!("{err}").is_empty());
    }

    #[test]
    fn unconfined_plan_opens_deno_with_allow_all() {
        let plan = build_unconfined_plan(DelegatingJsRuntimeOption::Deno);
        assert!(plan.args.iter().any(|a| a == "--allow-all"), "{plan:?}");
    }
}
