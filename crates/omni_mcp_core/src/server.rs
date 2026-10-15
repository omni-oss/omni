use std::{future::Future, sync::Mutex};

use bridge_rpc_services::{FsSys, ProcSys};
use omni_api::OmniApi;
use omni_context::{ConfigFingerprint, Context, ContextSys};
use omni_generator::GeneratorSys;
use omni_messages::{NoopSubscriber, OmniEventSubscriber};
use omni_task_executor::TaskExecutorSys;
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

/// MCP server backed by an omni workspace.
///
/// The server is long-lived, so before each tool call the stored [`Context`]
/// is checked against the on-disk workspace and remote cache configuration
/// files and rebuilt if any of them changed. Each tool call then creates a
/// fresh [`OmniApi`] from that context; project configurations are always
/// discovered and read from disk by the API itself. Tool operations use
/// [`NoopSubscriber`] so that their futures are unconditionally `Send`.
pub struct OmniMcpServer<TSys: ContextSys> {
    state: Mutex<ContextState<TSys>>,
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
            state: Mutex::new(ContextState { ctx, fingerprint }),
        }
    }

    /// Returns the current context, reloading it first if the workspace or
    /// remote cache configuration changed on disk since the last load. A
    /// failed reload is reported to the caller and retried on the next call.
    pub(crate) fn context(&self) -> eyre::Result<Context<TSys>> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
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
        info
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, rmcp::model::ErrorData> {
        Ok(ListToolsResult {
            tools: crate::tools::tool_list(),
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
        let args = args.unwrap_or(Value::Object(Default::default()));
        match name {
            "workspace_info" => call0(self.tool_workspace_info()).await,
            "project_list" => call0(self.tool_project_list()).await,
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
