//! The code table, generated from the registry.

use std::fmt::Write as _;

use serde_json::{Value, json};

use crate::{Audience, registrations};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub code: &'static str,
    pub audience: Audience,
    pub type_name: &'static str,
}

/// Every registered code, sorted by code.
pub fn entries() -> Vec<Entry> {
    let mut v: Vec<Entry> = registrations()
        .flat_map(|r| {
            r.codes.iter().map(|c| Entry {
                code: c.code,
                audience: c.audience,
                type_name: r.type_name,
            })
        })
        .collect();
    v.sort_by(|a, b| a.code.cmp(b.code).then(a.type_name.cmp(b.type_name)));
    v
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Text,
    Json,
    Markdown,
}

pub fn render(format: Format) -> String {
    let entries = entries();
    match format {
        Format::Json => {
            let v: Vec<Value> = entries
                .iter()
                .map(|e| {
                    json!({
                        "code": e.code,
                        "audience": e.audience,
                        "type": e.type_name,
                    })
                })
                .collect();
            serde_json::to_string_pretty(&v).unwrap_or_default()
        }
        Format::Text => {
            let w = entries.iter().map(|e| e.code.len()).max().unwrap_or(0);
            let mut out = String::new();
            for e in &entries {
                let _ = writeln!(out, "{:<w$}  {}", e.code, e.audience);
            }
            out
        }
        Format::Markdown => {
            let mut out =
                String::from("| Code | Audience |\n| ---- | -------- |\n");
            for e in &entries {
                let _ = writeln!(out, "| `{}` | {} |", e.code, e.audience);
            }
            out
        }
    }
}
