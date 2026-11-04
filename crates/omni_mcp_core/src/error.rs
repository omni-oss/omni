use omni_diagnostic::{Audience, DiagnosticReport, render::chain_messages};
use rmcp::model::{ErrorCode, ErrorData as McpError};
use serde_json::json;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct Error(pub(crate) ErrorInner);

impl Error {
    pub fn custom(message: impl Into<String>) -> Self {
        Self(ErrorInner::Custom(eyre::Report::msg(message.into())))
    }

    pub fn into_mcp_error(self) -> McpError {
        report_to_mcp_error(eyre::Report::new(self))
    }

    #[allow(unused)]
    pub fn kind(&self) -> ErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<ErrorInner>> From<T> for Error {
    fn from(inner: T) -> Self {
        Self(inner.into())
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(vis(pub), name(ErrorKind))]
pub(crate) enum ErrorInner {
    #[error(transparent)]
    Custom(#[from] eyre::Report),
}

/// Maps any failure to an MCP error (RFC 0021, section 4.3).
///
/// A diagnostic keeps its structured envelope as `data`; user mistakes are
/// `invalid_params`, everything else `internal_error`. Other errors keep their
/// full cause chain instead of only the outermost message.
pub fn report_to_mcp_error(report: eyre::Report) -> McpError {
    if let Some(failure) = report.downcast_ref::<McpFailure>() {
        return failure.0.clone();
    }
    let Some(d) = DiagnosticReport::from_report(&report) else {
        return McpError::internal_error(
            format!("{report:#}"),
            Some(json!({ "context": chain_messages(&report) })),
        );
    };
    let d = d.redact_url_userinfo();
    let code = match d.audience {
        Audience::User => ErrorCode::INVALID_PARAMS,
        Audience::Environment | Audience::Internal => ErrorCode::INTERNAL_ERROR,
    };
    let data = d.to_json();
    McpError::new(code, d.message, Some(data))
}

/// An already-built MCP error, carried through `eyre` so a tool can attach
/// request-scoped data (notices) to the payload.
#[derive(Debug)]
pub(crate) struct McpFailure(McpError);

impl std::fmt::Display for McpFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0.message)
    }
}

impl std::error::Error for McpFailure {}

/// Attaches the notices collected before a failure to the error payload as
/// `data.notices` (RFC 0023, 4.5). Notices stay true about the run even when
/// a later, unrelated error ends it.
pub(crate) fn with_notices(
    report: eyre::Report,
    notices: Vec<crate::notices::NoticePayload>,
) -> eyre::Report {
    if notices.is_empty() {
        return report;
    }
    let mut e = report_to_mcp_error(report);
    let mut data = match e.data.take() {
        Some(serde_json::Value::Object(o)) => o,
        Some(other) => {
            let mut m = serde_json::Map::new();
            m.insert("detail".into(), other);
            m
        }
        None => serde_json::Map::new(),
    };
    data.insert("notices".into(), json!(notices));
    e.data = Some(serde_json::Value::Object(data));
    eyre::Report::new(McpFailure(e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notices_are_attached_under_data_on_failure() {
        use crate::notices::NoticePayload;
        let notice = NoticePayload {
            level: "warn".into(),
            code: "x.y".into(),
            message: "m".into(),
            fields: Default::default(),
        };
        let r = with_notices(eyre::Report::msg("boom"), vec![notice]);
        let e = report_to_mcp_error(r);
        assert_eq!(e.message, "boom");
        assert_eq!(e.data.unwrap()["notices"][0]["code"], "x.y");
    }

    #[test]
    fn no_notices_leaves_the_error_untouched() {
        let r = with_notices(eyre::Report::msg("boom"), vec![]);
        let e = report_to_mcp_error(r);
        assert!(e.data.unwrap().get("notices").is_none());
    }

    #[test]
    fn plain_errors_keep_their_cause_chain() {
        use eyre::WrapErr;
        let r = Err::<(), _>(std::io::Error::other("disk full"))
            .wrap_err("writing cache")
            .unwrap_err();
        let e = report_to_mcp_error(r);
        assert_eq!(e.code, ErrorCode::INTERNAL_ERROR);
        assert_eq!(e.message, "writing cache: disk full");
        assert_eq!(
            e.data.unwrap()["context"],
            json!(["writing cache", "disk full"])
        );
    }
}
