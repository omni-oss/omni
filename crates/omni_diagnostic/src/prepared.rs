//! Errors prepared for display after their operation's secrets are known
//! (RFC 0026).
//!
//! An operation owns an immutable redactor only while it runs. An error that
//! leaves it must not need that redactor later, so [`prepare_report`] builds a
//! sanitized stand-in first. The stand-in keeps the diagnostic's code,
//! audience, structured details, hints and context chain, and contains no
//! raw source error and no redactor.

use std::{error::Error, fmt};

use serde_json::Value;

use crate::{Audience, Diagnostic, DiagnosticReport};

/// A diagnostic whose every text field was sanitized when it was built.
#[derive(Debug)]
pub struct PreparedDiagnostic {
    code: &'static str,
    audience: Audience,
    message: String,
    details: Option<Value>,
    body: Option<String>,
    hints: Vec<String>,
    causes: Option<Box<TextCause>>,
}

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

impl PreparedDiagnostic {
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

impl fmt::Display for PreparedDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for PreparedDiagnostic {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.causes.as_deref().map(|c| c as &(dyn Error + 'static))
    }
}

impl Diagnostic for PreparedDiagnostic {
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

/// Sanitizes every text a user can see in `report` with `redact`: the
/// diagnostic's message, body, hints, causes and structured details, and each
/// context layer. Returns `report` itself when nothing changes, so downcasts
/// of an untouched error keep working.
pub fn prepare_report(
    report: eyre::Report,
    redact: &dyn Fn(&str) -> String,
) -> eyre::Report {
    if let Some(d) = DiagnosticReport::from_report(&report) {
        let safe = d.map_text(redact);
        if safe == d {
            return report;
        }
        let mut out = eyre::Report::new(PreparedDiagnostic::new(&safe));
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
    #[diagnostic(code = "prepared.sample", hint = "check {0}")]
    struct Sample(String);

    fn hide(s: &str) -> String {
        s.replace("hunter2", "<redacted>")
    }

    #[test]
    fn a_diagnostic_keeps_its_structure_and_loses_the_secret() {
        let report = eyre::Report::new(Sample("hunter2".into()))
            .wrap_err("running hunter2 task");
        let safe = prepare_report(report, &hide);

        let d =
            DiagnosticReport::from_report(&safe).expect("still a diagnostic");
        assert_eq!(d.code, "prepared.sample");
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
        let safe = prepare_report(report, &hide);
        let layers: Vec<_> = safe.chain().map(|e| e.to_string()).collect();
        assert_eq!(layers, ["top <redacted>", "middle", "root <redacted>"]);
    }

    #[test]
    fn an_untouched_error_is_returned_as_is() {
        let report = eyre::Report::new(Sample("fine".into()));
        let safe = prepare_report(report, &hide);
        assert!(safe.downcast_ref::<Sample>().is_some());
        let plain = prepare_report(eyre::eyre!("fine"), &hide);
        assert_eq!(plain.to_string(), "fine");
    }
}
