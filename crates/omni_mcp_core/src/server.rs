use std::future::Future;

use omni_api::OmniApi;
use omni_context::{Context, ContextSys};
use omni_generator::GeneratorSys;
use omni_messages::{NoopSubscriber, OmniEventSubscriber};
use omni_task_executor::TaskExecutorSys;
use rmcp::{
    ServerHandler,
    model::{
        CallToolRequestParams, CallToolResult, ListToolsResult,
        PaginatedRequestParams, ServerInfo,
    },
    service::{RequestContext, RoleServer},
};
use serde::Serialize;
use serde_json::Value;

/// MCP server backed by an omni workspace.
///
/// Each tool call creates a fresh [`OmniApi`] from a stored `Context`, ensuring
/// workspace files are always read from disk and never stale. The stored
/// subscriber is used only at the server level; tool operations always use
/// [`NoopSubscriber`] so that their futures are unconditionally `Send`.
pub struct OmniMcpServer<TSys: ContextSys> {
    pub(crate) ctx: Context<TSys>,
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
        Self { ctx }
    }

    /// Creates a fresh API for each tool call using [`NoopSubscriber`], ensuring
    /// that the resulting futures are unconditionally `Send`.
    pub(crate) fn make_api(&self) -> OmniApi<TSys, NoopSubscriber> {
        OmniApi::new_with_sys(self.ctx.clone(), NoopSubscriber)
    }

    pub(crate) fn make_api_with_subscriber<TSub: OmniEventSubscriber>(
        &self,
        subscriber: TSub,
    ) -> OmniApi<TSys, TSub> {
        OmniApi::new_with_sys(self.ctx.clone(), subscriber)
    }
}

impl<TSys> ServerHandler for OmniMcpServer<TSys>
where
    TSys: ContextSys
        + GeneratorSys
        + TaskExecutorSys
        + Clone
        + Send
        + Sync
        + 'static,
{
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.server_info = rmcp::model::Implementation::new(
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
        );
        info.capabilities.tools = Some(rmcp::model::ToolsCapability::default());
        info
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, rmcp::model::ErrorData>>
    + Send
    + '_ {
        async move {
            Ok(ListToolsResult {
                tools: crate::tools::tool_list(),
                ..Default::default()
            })
        }
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<CallToolResult, rmcp::model::ErrorData>>
    + Send
    + '_ {
        async move {
            self.dispatch(
                &request.name,
                request.arguments.map(serde_json::Value::Object),
            )
            .await
        }
    }
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
    async fn dispatch(
        &self,
        name: &str,
        args: Option<Value>,
    ) -> Result<CallToolResult, rmcp::model::ErrorData> {
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
) -> Result<CallToolResult, rmcp::model::ErrorData> {
    match fut.await {
        Ok(result) => {
            let value = serde_json::to_value(result).map_err(|e| {
                rmcp::model::ErrorData::internal_error(
                    format!("serialization error: {e}"),
                    None,
                )
            })?;
            Ok(CallToolResult::structured(value))
        }
        Err(e) => {
            Err(rmcp::model::ErrorData::internal_error(e.to_string(), None))
        }
    }
}

async fn call1<P, R, F, Fut>(
    args: Value,
    f: F,
) -> Result<CallToolResult, rmcp::model::ErrorData>
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
