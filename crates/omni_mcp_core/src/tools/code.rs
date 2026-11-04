use omni_context::ContextSys;
use omni_generator::GeneratorSys;
use omni_task_executor::TaskExecutorSys;
use serde_json::Value;

use crate::{
    codes::unknown_code,
    model::{CodeExplainParams, CodeListKind, CodeListParams, CodeListResult},
    server::OmniMcpServer,
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

    /// Lists the registry: the same entries as `omni code list --format json`.
    pub(crate) async fn tool_code_list(
        &self,
        params: CodeListParams,
    ) -> eyre::Result<CodeListResult> {
        Ok(code_list(params.kind))
    }
}

pub(crate) fn code_list(kind: Option<CodeListKind>) -> CodeListResult {
    let kind = kind.map(|k| match k {
        CodeListKind::Error => omni_codes::CodeKind::Error,
        CodeListKind::Notice => omni_codes::CodeKind::Notice,
    });
    CodeListResult {
        codes: omni_codes::catalog::entries()
            .into_iter()
            .filter(|c| kind.is_none_or(|k| c.kind() == k))
            .map(|c| c.to_json())
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_filters_by_kind_and_matches_the_registry() {
        let all = code_list(None).codes;
        assert!(all.len() >= 2);
        let errors = code_list(Some(CodeListKind::Error)).codes;
        let notices = code_list(Some(CodeListKind::Notice)).codes;
        assert_eq!(errors.len() + notices.len(), all.len());
        assert!(errors.iter().all(|c| c["kind"] == "error"));
        assert!(notices.iter().all(|c| c["kind"] == "notice"));
        let codes: Vec<_> =
            all.iter().map(|c| c["code"].as_str().unwrap()).collect();
        let mut sorted = codes.clone();
        sorted.sort();
        assert_eq!(codes, sorted);
    }
}

pub(crate) fn code_json(
    code: &str,
) -> Result<Value, crate::codes::UnknownCode> {
    omni_codes::lookup(code)
        .map(|c| c.to_json())
        .map_err(unknown_code)
}
