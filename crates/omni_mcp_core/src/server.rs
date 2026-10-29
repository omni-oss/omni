use std::{future::Future, path::PathBuf, sync::Mutex};

use crate::model::*;
use bridge_rpc_services::{FsSys, ProcSys};
use omni_api::OmniApi;
use omni_context::{
    ConfigFingerprint, Context, ContextSys, WorkspaceInitConfig,
    get_root_dir_from,
};
use omni_generator::GeneratorSys;
use omni_messages::{NoopSubscriber, OmniEventSubscriber};
use omni_task_executor::TaskExecutorSys;
use omni_tracing_subscriber::TracingConfig;
use rmcp::{
    ServerHandler,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult,
        ListToolsResult, PaginatedRequestParams, ServerConfig,
    },
    service::{RequestContext, RoleServer},
};
use serde::Serialize;
use serde_json::Value;
use system_traits::BaseFsMetadataAsync;

/// MCP server bound to one workspace or resolving a workspace per shared call.
///
/// In bound mode, before each tool call the stored [`Context`]
/// is checked against the on-disk workspace and remote cache configuration
/// files and rebuilt if any of them changed. Each tool call then creates a
/// fresh [`OmniApi`] from that context; project configurations are always
/// discovered and read from disk by the API itself. Tool operations use
/// [`NoopSubscriber`] so that their futures are unconditionally `Send`.
/// Shared mode stores initialization policy only and constructs a fresh,
/// request-owned adapter for every call without mutating process CWD or env.
pub struct OmniMcpServer<TSys: ContextSys> {
    state: ServerState<TSys>,
}

enum ServerState<TSys: ContextSys> {
    Bound(Box<Mutex<ContextState<TSys>>>),
    Shared {
        sys: TSys,
        config: WorkspaceInitConfig,
        tracing: TracingConfig,
    },
}

struct ContextState<TSys: ContextSys> {
    ctx: Context<TSys>,
    fingerprint: ConfigFingerprint,
}

impl<TSys> OmniMcpServer<TSys>
where
    TSys: ContextSys
        + GeneratorSys
        + TaskExecutorSys
        + Clone
        + Send
        + Sync
        + 'static,
{
    pub fn new(ctx: Context<TSys>) -> Self {
        let fingerprint = ctx.config_fingerprint();
        Self {
            state: ServerState::Bound(Box::new(Mutex::new(ContextState {
                ctx,
                fingerprint,
            }))),
        }
    }

    /// Construct a server without discovering or retaining a workspace.
    pub fn new_shared(
        sys: TSys,
        config: WorkspaceInitConfig,
        tracing: TracingConfig,
    ) -> Self {
        Self {
            state: ServerState::Shared {
                sys,
                config,
                tracing,
            },
        }
    }

    fn is_shared(&self) -> bool {
        matches!(self.state, ServerState::Shared { .. })
    }

    /// Returns the current context, reloading it first if the workspace or
    /// remote cache configuration changed on disk since the last load. A
    /// failed reload is reported to the caller and retried on the next call.
    pub(crate) fn context(&self) -> eyre::Result<Context<TSys>> {
        let ServerState::Bound(state) = &self.state else {
            eyre::bail!("shared server requires a request-local working_dir");
        };
        let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
        let fingerprint = state.ctx.config_fingerprint();

        if fingerprint != state.fingerprint {
            let ctx = state.ctx.reload()?;
            trace::info!("workspace configuration changed, context reloaded");
            *state = ContextState { ctx, fingerprint };
        }

        Ok(state.ctx.clone())
    }

    /// Creates a fresh API for each tool call using [`NoopSubscriber`], ensuring
    /// that the resulting futures are unconditionally `Send`.
    pub(crate) fn make_api(
        &self,
    ) -> eyre::Result<OmniApi<TSys, NoopSubscriber>> {
        Ok(OmniApi::new_with_sys(self.context()?, NoopSubscriber))
    }

    pub(crate) fn make_api_with_subscriber<TSub: OmniEventSubscriber>(
        &self,
        subscriber: TSub,
    ) -> eyre::Result<OmniApi<TSys, TSub>> {
        Ok(OmniApi::new_with_sys(self.context()?, subscriber))
    }
}

impl<TSys> ServerHandler for OmniMcpServer<TSys>
where
    TSys: ContextSys
        + GeneratorSys
        + TaskExecutorSys
        + FsSys
        + ProcSys
        + omni_api::ProjectionSys
        + Clone
        + Send
        + Sync
        + 'static,
    <TSys as BaseFsMetadataAsync>::Metadata: Send,
{
    fn get_info(&self) -> ServerConfig {
        let mut info = ServerConfig::default();
        info.server_info = rmcp::model::Implementation::new(
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
        );
        info.capabilities.tools = Some(rmcp::model::ToolsCapability::default());
        if self.is_shared() {
            info.instructions = Some("Every tool requires an absolute existing working_dir. Omni discovers the nearest workspace from the canonical physical directory. Each call is independent; no workspace selection is retained. Shared mode is not a filesystem sandbox.".into());
        }
        info
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, rmcp::model::ErrorData> {
        Ok(ListToolsResult {
            tools: if self.is_shared() {
                crate::tools::tool_list_for_mode(true)
            } else {
                crate::tools::tool_list()
            },
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, rmcp::model::ErrorData> {
        self.dispatch(
            &request.name,
            request.arguments.map(serde_json::Value::Object),
        )
        .await
    }
}

impl<TSys> OmniMcpServer<TSys>
where
    TSys: ContextSys
        + GeneratorSys
        + TaskExecutorSys
        + FsSys
        + ProcSys
        + omni_api::ProjectionSys
        + Clone
        + Send
        + Sync
        + 'static,
    <TSys as BaseFsMetadataAsync>::Metadata: Send,
{
    async fn dispatch(
        &self,
        name: &str,
        args: Option<Value>,
    ) -> Result<CallToolResponse, rmcp::model::ErrorData> {
        let mut args = args.unwrap_or(Value::Object(Default::default()));
        if let ServerState::Shared {
            sys,
            config,
            tracing,
        } = &self.state
        {
            // Typed deserialization validates both selection and operation shape
            // before any workspace is loaded. Unknown tools never trigger discovery.
            let dir = shared_working_dir(name, args.clone())?;
            if !dir.is_absolute() {
                return Err(rmcp::model::ErrorData::invalid_params(
                    "working_dir must be an absolute existing directory",
                    None,
                ));
            }
            let dir = sys
                .fs_canonicalize(&dir)
                .and_then(|dir| {
                    if sys.fs_is_dir(&dir)? {
                        Ok(dir)
                    } else {
                        Err(std::io::Error::other("not a directory"))
                    }
                })
                .map_err(|e| {
                    rmcp::model::ErrorData::invalid_params(
                        format!("invalid working_dir {}: {e}", dir.display()),
                        None,
                    )
                })?;
            let ctx = (|| -> eyre::Result<_> {
                let root = get_root_dir_from(sys, &dir)?;
                Ok(Context::new_with_working_dir(
                    sys.clone(),
                    &config.env,
                    &root,
                    config.inherit_env_vars,
                    &config.env_root_dir_marker,
                    config.env_files.clone(),
                    tracing,
                    config.cache_dir.clone(),
                    Some(dir.clone()),
                )?)
            })()
            .map_err(|e| {
                rmcp::model::ErrorData::internal_error(
                    format!("working_dir {}: {e:#}", dir.display()),
                    None,
                )
            })?;
            // The request owns this bound adapter. Both normal and subscriber-
            // backed APIs therefore use exactly the same resolved context.
            if name == "tool_run" {
                args["working_dir"] =
                    serde_json::to_value(&dir).map_err(|e| {
                        rmcp::model::ErrorData::invalid_params(
                            e.to_string(),
                            None,
                        )
                    })?;
            }
            return Self::new(ctx).dispatch_bound(name, args).await;
        }
        self.dispatch_bound(name, args).await
    }

    async fn dispatch_bound(
        &self,
        name: &str,
        args: Value,
    ) -> Result<CallToolResponse, rmcp::model::ErrorData> {
        match name {
            "workspace_info" => call0(self.tool_workspace_info()).await,
            "project_list" => call1(args, |p| self.tool_project_list(p)).await,
            "project_config" => {
                call1(args, |p| self.tool_project_config(p)).await
            }
            "generator_list" => call0(self.tool_generator_list()).await,
            "generator_inspect" => {
                call1(args, |p| self.tool_generator_inspect(p)).await
            }
            "generator_run" => {
                call1(args, |p| self.tool_generator_run(p)).await
            }
            "generator_validate_input" => {
                call1(args, |p| self.tool_generator_validate_input(p)).await
            }
            "hash_workspace" => call0(self.tool_hash_workspace()).await,
            "hash_project" => call1(args, |p| self.tool_hash_project(p)).await,
            "cache_stats" => call1(args, |p| self.tool_cache_stats(p)).await,
            "cache_prune" => call1(args, |p| self.tool_cache_prune(p)).await,
            "task_run" => call1(args, |p| self.tool_task_run(p)).await,
            "exec_command" => call1(args, |p| self.tool_exec_command(p)).await,
            "tool_list" => call0(self.tool_tool_list()).await,
            "tool_inspect" => call1(args, |p| self.tool_tool_inspect(p)).await,
            "tool_run" => call1(args, |p| self.tool_tool_run(p)).await,
            "projection_sync" => {
                call1(args, |p| self.tool_projection_sync(p)).await
            }
            "projection_status" => {
                call1(args, |p| self.tool_projection_status(p)).await
            }
            "projection_unlink" => {
                call1(args, |p| self.tool_projection_unlink(p)).await
            }
            "projection_prune" => {
                call1(args, |p| self.tool_projection_prune(p)).await
            }
            "ignore_sync" => call1(args, |p| self.tool_ignore_sync(p)).await,
            "ignore_clean" => call1(args, |p| self.tool_ignore_clean(p)).await,
            unknown => Err(rmcp::model::ErrorData::new(
                rmcp::model::ErrorCode::METHOD_NOT_FOUND,
                format!("unknown tool: {unknown}"),
                None,
            )),
        }
    }
}

fn shared_working_dir(
    name: &str,
    args: Value,
) -> Result<PathBuf, rmcp::model::ErrorData> {
    fn parse<P: serde::de::DeserializeOwned>(
        args: Value,
    ) -> Result<PathBuf, rmcp::model::ErrorData> {
        let params: WorkspaceParams<P> = serde_json::from_value(args).map_err(|e| {
            rmcp::model::ErrorData::invalid_params(format!("invalid params (working_dir must be an absolute existing directory): {e}"), None)
        })?;
        Ok(params.working_dir)
    }
    match name {
        "workspace_info" | "generator_list" | "hash_workspace"
        | "tool_list" => parse::<EmptyParams>(args),
        "project_list" => parse::<ProjectListParams>(args),
        "project_config" => parse::<ProjectConfigParams>(args),
        "generator_inspect" => parse::<GeneratorInspectParams>(args),
        "generator_run" => parse::<GeneratorRunParams>(args),
        "generator_validate_input" => {
            parse::<GeneratorValidateInputParams>(args)
        }
        "hash_project" => parse::<HashProjectParams>(args),
        "cache_stats" => parse::<CacheStatsParams>(args),
        "cache_prune" => parse::<CachePruneParams>(args),
        "task_run" => parse::<TaskRunParams>(args),
        "exec_command" => parse::<ExecCommandParams>(args),
        "tool_inspect" => parse::<ToolInspectParams>(args),
        "tool_run" => parse::<SharedToolRunParams>(args),
        "projection_sync" => parse::<ProjectionSyncParams>(args),
        "projection_status" => parse::<ProjectionStatusParams>(args),
        "projection_unlink" => parse::<ProjectionUnlinkParams>(args),
        "projection_prune" => parse::<ProjectionPruneParams>(args),
        "ignore_sync" => parse::<IgnoreSyncParams>(args),
        "ignore_clean" => parse::<IgnoreCleanParams>(args),
        unknown => Err(rmcp::model::ErrorData::new(
            rmcp::model::ErrorCode::METHOD_NOT_FOUND,
            format!("unknown tool: {unknown}"),
            None,
        )),
    }
}

async fn call0<R: Serialize>(
    fut: impl Future<Output = eyre::Result<R>>,
) -> Result<CallToolResponse, rmcp::model::ErrorData> {
    match fut.await {
        Ok(result) => {
            let value = serde_json::to_value(result).map_err(|e| {
                rmcp::model::ErrorData::internal_error(
                    format!("serialization error: {e}"),
                    None,
                )
            })?;
            Ok(CallToolResponse::Complete(CallToolResult::structured(
                value,
            )))
        }
        Err(e) => {
            Err(rmcp::model::ErrorData::internal_error(e.to_string(), None))
        }
    }
}

async fn call1<P, R, F, Fut>(
    args: Value,
    f: F,
) -> Result<CallToolResponse, rmcp::model::ErrorData>
where
    P: serde::de::DeserializeOwned,
    R: Serialize,
    F: FnOnce(P) -> Fut,
    Fut: Future<Output = eyre::Result<R>>,
{
    let params: P = serde_json::from_value(args).map_err(|e| {
        rmcp::model::ErrorData::invalid_params(
            format!("invalid params: {e}"),
            None,
        )
    })?;
    call0(f(params)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn concurrent_shared_calls_leave_process_directory_and_environment_unchanged()
     {
        let tmp = tempfile::tempdir().unwrap();
        for name in ["alpha", "beta"] {
            let dir = tmp.path().join(name);
            std::fs::create_dir(&dir).unwrap();
            std::fs::write(dir.join("workspace.omni.yaml"), "projects: []\n")
                .unwrap();
            std::fs::write(
                dir.join(".env"),
                format!("OMNI_SHARED_ISOLATION_MARKER={name}\n"),
            )
            .unwrap();
        }
        let cwd = std::env::current_dir().unwrap();
        let env =
            std::env::vars_os().collect::<std::collections::BTreeMap<_, _>>();
        let server = OmniMcpServer::new_shared(
            system_traits::impls::RealSys,
            WorkspaceInitConfig::default(),
            TracingConfig::disabled(),
        );
        let call = |name: &'static str| {
            let server = &server;
            let dir = tmp.path().join(name);
            async move {
                let result = server
                    .dispatch(
                        "workspace_info",
                        Some(json!({"working_dir": dir})),
                    )
                    .await
                    .unwrap();
                let CallToolResponse::Complete(result) = result else {
                    panic!("unexpected task response")
                };
                assert_eq!(
                    result.structured_content.unwrap()["env_vars"]["OMNI_SHARED_ISOLATION_MARKER"],
                    name
                );
            }
        };
        tokio::join!(call("alpha"), call("beta"));
        assert_eq!(std::env::current_dir().unwrap(), cwd);
        assert_eq!(
            std::env::vars_os().collect::<std::collections::BTreeMap<_, _>>(),
            env
        );
    }

    #[test]
    fn shared_shapes_are_validated_without_workspace_discovery() {
        for tool in crate::tools::tool_list_for_mode(true) {
            let error = shared_working_dir(&tool.name, json!({})).unwrap_err();
            assert_eq!(error.code, rmcp::model::ErrorCode::INVALID_PARAMS);
            assert!(error.message.contains("working_dir"));
        }
        assert!(
            shared_working_dir(
                "workspace_info",
                json!({"working_dir": "/workspace"})
            )
            .is_ok()
        );
        assert!(
            shared_working_dir(
                "generator_inspect",
                json!({"working_dir": "/workspace"})
            )
            .unwrap_err()
            .message
            .contains("name")
        );
        assert_eq!(
            shared_working_dir("unknown", json!({})).unwrap_err().code,
            rmcp::model::ErrorCode::METHOD_NOT_FOUND
        );
    }

    #[test]
    fn flattened_tool_run_has_one_directory_and_preserves_defaults() {
        let params: WorkspaceParams<SharedToolRunParams> =
            serde_json::from_value(json!({
                "working_dir": "/workspace/sub", "name": "writer"
            }))
            .unwrap();
        assert_eq!(params.working_dir, PathBuf::from("/workspace/sub"));
        assert_eq!(params.params.name, "writer");
        assert_eq!(params.params.args, json!({}));
    }
}
