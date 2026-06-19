pub mod error;
pub mod model;
pub mod server;
mod tools;

pub use server::OmniMcpServer;

/// Serve an [`OmniMcpServer`] over stdio using the MCP protocol.
pub async fn serve_stdio<TSys, S>(
    server: OmniMcpServer<TSys, S>,
) -> eyre::Result<()>
where
    TSys: omni_context::ContextSys
        + omni_generator::GeneratorSys
        + omni_task_executor::TaskExecutorSys
        + Clone
        + Send
        + Sync
        + 'static,
    S: omni_messages::OmniEventSubscriber + Send + Sync + 'static,
{
    let transport = rmcp::transport::stdio();
    let running = rmcp::serve_server(server, transport).await?;
    running.waiting().await?;
    Ok(())
}
