//! The code table, rendered from the registry (RFC 0023). Backs
//! `omni code list` and the `code_list` MCP tool.

use std::fmt::Write as _;

use serde_json::Value;

use crate::{CodeInfo, all};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Text,
    Json,
    Markdown,
}

/// Every registered code, sorted by code.
pub fn entries() -> Vec<&'static CodeInfo> {
    let mut v: Vec<&'static CodeInfo> = all().collect();
    v.sort_by_key(|c| c.code);
    v
}

fn who(c: &CodeInfo) -> String {
    match (c.level(), c.audience()) {
        (Some(l), _) => l.as_str().to_string(),
        (_, Some(a)) => a.as_str().to_string(),
        _ => String::new(),
    }
}

pub fn render(format: Format) -> String {
    render_entries(&entries(), format)
}

pub fn render_entries(entries: &[&CodeInfo], format: Format) -> String {
    match format {
        Format::Json => {
            let v: Vec<Value> = entries.iter().map(|e| e.to_json()).collect();
            serde_json::to_string_pretty(&v).unwrap_or_default()
        }
        Format::Text => {
            let w = entries.iter().map(|e| e.code.len()).max().unwrap_or(0);
            let mut out = String::new();
            for e in entries {
                let _ = writeln!(
                    out,
                    "{:<w$}  {:<6}  {}",
                    e.code,
                    e.kind().as_str(),
                    who(e)
                );
            }
            out
        }
        Format::Markdown => {
            let mut out = String::from(
                "| Code | Kind | Level / audience | Essential | Stable | Summary |\n| ---- | ---- | ---------------- | --------- | ------ | ------- |\n",
            );
            for e in entries {
                let _ = writeln!(
                    out,
                    "| `{}` | {} | {} | {} | {} | {} |",
                    e.code,
                    e.kind().as_str(),
                    who(e),
                    if e.essential() { "yes" } else { "" },
                    if e.stable { "yes" } else { "" },
                    e.summary.replace('|', "\\|"),
                );
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Audience, CodeDetail, NoticeLevel};

    const A: CodeInfo = CodeInfo {
        code: "a.err",
        summary: "An error.",
        explanation: "",
        related: &[],
        stable: true,
        deprecated: false,
        detail: CodeDetail::Error {
            audience: Audience::User,
        },
    };
    const B: CodeInfo = CodeInfo {
        code: "a.note",
        summary: "A | note.",
        explanation: "",
        related: &[],
        stable: false,
        deprecated: false,
        detail: CodeDetail::Notice {
            level: NoticeLevel::Warn,
            essential: true,
        },
    };

    #[test]
    fn markdown_has_one_row_per_code_and_escapes_pipes() {
        let md = render_entries(&[&A, &B], Format::Markdown);
        assert_eq!(md.lines().count(), 4);
        assert!(md.contains("| `a.err` | error | user |  | yes | An error. |"));
        assert!(
            md.contains("| `a.note` | notice | warn | yes |  | A \\| note. |")
        );
    }

    #[test]
    fn json_keeps_the_code_key() {
        let json = render_entries(&[&A], Format::Json);
        let v: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(v[0]["code"], "a.err");
        assert_eq!(v[0]["kind"], "error");
    }
}
