use std::fmt::Write as _;

use serde_json::{Map, Value, json};

use crate::{Audience, Diagnostic, find_in_error};

/// A diagnostic captured together with the context layers above it, ready to
/// be rendered as CLI text or as the JSON envelope used by MCP and
/// `--error-format json`.
#[derive(Debug, Clone, PartialEq)]
pub struct DiagnosticReport {
    pub code: &'static str,
    pub audience: Audience,
    pub message: String,
    /// Messages added with `wrap_err` above the diagnostic, outermost first.
    pub context: Vec<String>,
    /// The diagnostic's own `source()` chain, outermost first.
    pub causes: Vec<String>,
    pub details: Option<Value>,
    pub body: Option<String>,
    pub hints: Vec<String>,
}

impl DiagnosticReport {
    pub fn new(diagnostic: &dyn Diagnostic, context: Vec<String>) -> Self {
        let mut body = String::new();
        let _ = diagnostic.render_body(&mut body);
        let mut causes = Vec::new();
        let mut src = diagnostic.source();
        while let Some(e) = src {
            causes.push(e.to_string());
            src = e.source();
        }
        Self {
            code: diagnostic.code(),
            audience: diagnostic.audience(),
            message: diagnostic.to_string(),
            context,
            causes,
            details: diagnostic.details(),
            body: (!body.trim().is_empty())
                .then(|| body.trim_end().to_string()),
            hints: diagnostic.hints(),
        }
    }

    /// Captures the outermost diagnostic in the report, if any.
    pub fn from_report(report: &eyre::Report) -> Option<Self> {
        let mut context = Vec::new();
        for e in report.chain() {
            if let Some(d) = find_in_error(e) {
                return Some(Self::new(d, context));
            }
            context.push(e.to_string());
        }
        None
    }

    pub fn exit_code(&self) -> i32 {
        self.audience.exit_code()
    }

    /// The JSON envelope (RFC 0021, section 4.4).
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("kind".into(), json!(self.code));
        m.insert("audience".into(), json!(self.audience));
        m.insert("message".into(), json!(self.message));
        m.insert("context".into(), json!(self.context));
        m.insert("causes".into(), json!(self.causes));
        if let Some(d) = &self.details {
            m.insert("details".into(), d.clone());
        }
        m.insert("hints".into(), json!(self.hints));
        Value::Object(m)
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "error[{}]: {}", self.code, self.message);
        for c in &self.context {
            let _ = writeln!(out, "  while {c}");
        }
        for c in &self.causes {
            let _ = writeln!(out, "  caused by: {c}");
        }
        if let Some(b) = &self.body {
            let _ = write!(out, "\n{b}\n");
        }
        if !self.hints.is_empty() {
            out.push('\n');
            for h in &self.hints {
                let _ = writeln!(out, "  hint: {h}");
            }
        }
        out
    }
}

/// Messages of every layer in a report's cause chain, outermost first.
/// Used for errors that are not diagnostics, so MCP keeps the full chain.
pub fn chain_messages(report: &eyre::Report) -> Vec<String> {
    report.chain().map(|e| e.to_string()).collect()
}
