use omni_context::ContextSys;
use omni_generator::GeneratorSys;
use omni_task_executor::TaskExecutorSys;
use serde_json::Value;

use crate::{codes::unknown_code, model::ExplainParams, server::OmniMcpServer};

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
    /// Returns the registry entry for a code: the same fields as
    /// `omni explain --format json`.
    pub(crate) async fn tool_explain(
        &self,
        params: ExplainParams,
    ) -> eyre::Result<Value> {
        Ok(explain_json(&params.code)?)
    }
}

pub(crate) fn explain_json(
    code: &str,
) -> Result<Value, crate::codes::UnknownCode> {
    omni_codes::explain(code)
        .map(|c| c.to_json())
        .map_err(unknown_code)
}
