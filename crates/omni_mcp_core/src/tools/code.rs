use omni_context::ContextSys;
use omni_generator::GeneratorSys;
use omni_task_executor::TaskExecutorSys;
use serde_json::Value;

use crate::{
    codes::unknown_code, model::CodeExplainParams, server::OmniMcpServer,
};

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
    /// `omni code explain --format json`.
    pub(crate) async fn tool_code_explain(
        &self,
        params: CodeExplainParams,
    ) -> eyre::Result<Value> {
        Ok(code_json(&params.code)?)
    }
}

pub(crate) fn code_json(
    code: &str,
) -> Result<Value, crate::codes::UnknownCode> {
    omni_codes::lookup(code)
        .map(|c| c.to_json())
        .map_err(unknown_code)
}
