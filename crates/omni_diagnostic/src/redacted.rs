//! Errors that have already been redacted (RFC 0026).
//!
//! An operation owns its redactor only while it runs, but its errors can be
//! shown long after. [`redact_report`] therefore redacts an error while the
//! secrets are still known and returns a copy that holds only the redacted
//! text. The copy keeps the diagnostic's code, audience, structured details,
//! hints and context chain, and holds neither the original source error nor a
//! redactor.

use std::{error::Error, fmt};

use serde_json::Value;

use crate::{Audience, Diagnostic, DiagnosticReport};

/// A diagnostic whose text fields were all redacted when it was built.
#[derive(Debug)]
pub struct RedactedDiagnostic {
    code: &'static str,
    audience: Audience,
    message: String,
    details: Option<Value>,
    body: Option<String>,
    hints: Vec<String>,
    causes: Option<Box<TextCause>>,
}

/// One redacted cause in a [`RedactedDiagnostic`]'s source chain.
#[derive(Debug)]
struct TextCause {
    message: String,
    next: Option<Box<TextCause>>,
}

impl fmt::Display for TextCause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for TextCause {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.next.as_deref().map(|c| c as &(dyn Error + 'static))
    }
}

impl RedactedDiagnostic {
    /// Captures an already-redacted report, rebuilding its cause chain from text.
    fn new(report: &DiagnosticReport) -> Self {
        let causes = report.causes.iter().rev().fold(None, |next, m| {
            Some(Box::new(TextCause {
                message: m.clone(),
                next,
            }))
        });
        Self {
            code: report.code,
            audience: report.audience,
            message: report.message.clone(),
            details: report.details.clone(),
            body: report.body.clone(),
            hints: report.hints.clone(),
            causes,
        }
    }
}

impl fmt::Display for RedactedDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for RedactedDiagnostic {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.causes.as_deref().map(|c| c as &(dyn Error + 'static))
    }
}

impl Diagnostic for RedactedDiagnostic {
    fn code(&self) -> &'static str {
        self.code
    }

    fn audience(&self) -> Audience {
        self.audience
    }

    fn details(&self) -> Option<Value> {
        self.details.clone()
    }

    fn hints(&self) -> Vec<String> {
        self.hints.clone()
    }

    fn render_body(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        match &self.body {
            Some(b) => out.write_str(b),
            None => Ok(()),
        }
    }
}

/// Applies `redact` to the diagnostic's message, body, hints, causes, context
/// layers, and string values in structured details. Detail keys are unchanged.
/// For a plain error, applies it to each displayed chain message. Returns
/// `report` itself when nothing changes, preserving downcasts; otherwise the
/// returned report retains diagnostic structure but not the original error type.
pub fn redact_report(
    report: eyre::Report,
    redact: &dyn Fn(&str) -> String,
) -> eyre::Report {
    if let Some(d) = DiagnosticReport::from_report(&report) {
        let safe = d.map_text(redact);
        if safe == d {
            return report;
        }
        let mut out = eyre::Report::new(RedactedDiagnostic::new(&safe));
        for ctx in safe.context.into_iter().rev() {
            out = out.wrap_err(ctx);
        }
        return out;
    }

    let messages: Vec<String> = report.chain().map(|e| e.to_string()).collect();
    let safe: Vec<String> = messages.iter().map(|m| redact(m)).collect();
    if safe == messages {
        return report;
    }
    let mut layers = safe.into_iter().rev();
    let mut out = eyre::Report::msg(layers.next().unwrap_or_default());
    for layer in layers {
        out = out.wrap_err(layer);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, thiserror::Error, crate::Diagnostic)]
    #[error("cannot use {0}")]
    #[diagnostic(code = "redacted.sample", hint = "check {0}")]
    struct Sample(String);

    fn hide(s: &str) -> String {
        s.replace("hunter2", "<redacted>")
    }

    #[test]
    fn a_diagnostic_keeps_its_structure_and_loses_the_secret() {
        let report = eyre::Report::new(Sample("hunter2".into()))
            .wrap_err("running hunter2 task");
        let safe = redact_report(report, &hide);

        let d =
            DiagnosticReport::from_report(&safe).expect("still a diagnostic");
        assert_eq!(d.code, "redacted.sample");
        assert_eq!(d.audience, Audience::User);
        assert_eq!(d.message, "cannot use <redacted>");
        assert_eq!(d.hints, vec!["check <redacted>".to_string()]);
        assert_eq!(d.context, vec!["running <redacted> task".to_string()]);
        let all = format!("{} {} {safe:?} {safe:#}", d.to_text(), d.to_json());
        assert!(!all.contains("hunter2"), "{all}");
    }

    #[test]
    fn a_plain_error_chain_is_sanitized_layer_by_layer() {
        let report = eyre::eyre!("root hunter2")
            .wrap_err("middle")
            .wrap_err("top hunter2");
        let safe = redact_report(report, &hide);
        let layers: Vec<_> = safe.chain().map(|e| e.to_string()).collect();
        assert_eq!(layers, ["top <redacted>", "middle", "root <redacted>"]);
    }

    #[test]
    fn an_untouched_error_is_returned_as_is() {
        let report = eyre::Report::new(Sample("fine".into()));
        let safe = redact_report(report, &hide);
        assert!(safe.downcast_ref::<Sample>().is_some());
        let plain = redact_report(eyre::eyre!("fine"), &hide);
        assert_eq!(plain.to_string(), "fine");
    }
}
