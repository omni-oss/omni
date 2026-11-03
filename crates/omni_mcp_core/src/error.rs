use omni_diagnostic::{Audience, DiagnosticReport, render::chain_messages};
use rmcp::model::{ErrorCode, ErrorData as McpError};
use serde_json::{Value, json};
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
    let Some(d) = DiagnosticReport::from_report(&report) else {
        return McpError::internal_error(
            format!("{report:#}"),
            Some(json!({ "context": chain_messages(&report) })),
        );
    };
    let code = match d.audience {
        Audience::User => ErrorCode::INVALID_PARAMS,
        Audience::Environment | Audience::Internal => ErrorCode::INTERNAL_ERROR,
    };
    let mut data = d.to_json();
    if d.code == "meta_filter.evaluation" {
        add_legacy_meta_filter_fields(&mut data);
    }
    McpError::new(code, d.message, Some(data))
}

/// RFC 0020 clients read `filter`/`total`/`other`/`groups` from the top level
/// of `data`. Kept for one release, then removed (RFC 0021, section 4.4).
fn add_legacy_meta_filter_fields(data: &mut Value) {
    let Some(details) = data.get("details").and_then(Value::as_object).cloned()
    else {
        return;
    };
    if let Some(top) = data.as_object_mut() {
        for (k, v) in details {
            top.entry(k).or_insert(v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
