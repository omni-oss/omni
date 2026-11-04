//! Logic shared by `omni_diagnostic_derive` and `omni_notice_derive`.
//!
//! A proc-macro crate cannot export non-macro items, so the shared parsing and
//! emission live here.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Attribute, Expr, ExprLit, Lit, LitStr, Meta};

/// `true` for lowercase `snake_case` segments separated by dots, at least two.
pub fn valid_code(code: &str) -> bool {
    let mut segments = 0;
    for seg in code.split('.') {
        segments += 1;
        let mut chars = seg.chars();
        match chars.next() {
            Some(c) if c.is_ascii_lowercase() => {}
            _ => return false,
        }
        if !chars
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            return false;
        }
    }
    segments >= 2
}

/// Splits the `///` doc comment into `(summary, explanation)`.
///
/// The summary is the first paragraph collapsed to one line; the explanation
/// is everything after the first blank line.
pub fn doc_parts(attrs: &[Attribute]) -> (String, String) {
    let mut lines: Vec<String> = Vec::new();
    for a in attrs {
        if !a.path().is_ident("doc") {
            continue;
        }
        if let Meta::NameValue(nv) = &a.meta
            && let Expr::Lit(ExprLit {
                lit: Lit::Str(s), ..
            }) = &nv.value
        {
            let v = s.value();
            lines.push(v.strip_prefix(' ').unwrap_or(&v).trim_end().to_string());
        }
    }
    while lines.first().is_some_and(|l| l.is_empty()) {
        lines.remove(0);
    }
    let split = lines.iter().position(|l| l.is_empty());
    let (head, tail) = match split {
        Some(i) => (&lines[..i], &lines[i + 1..]),
        None => (&lines[..], &lines[0..0]),
    };
    let summary = head.join(" ").trim().to_string();
    let explanation = tail.join("\n").trim().to_string();
    (summary, explanation)
}

/// `true` if a `format!`-style template contains a placeholder (a `{` that
/// is not the escape `{{`).
pub fn has_placeholder(template: &str) -> bool {
    let b = template.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'{' if b.get(i + 1) == Some(&b'{') => i += 2,
            b'{' => return true,
            _ => i += 1,
        }
    }
    false
}

/// Everything needed to emit a `CodeInfo` expression.
pub struct CodeInfoTokens<'a> {
    pub code: &'a LitStr,
    pub summary: &'a str,
    pub explanation: TokenStream,
    pub related: &'a [LitStr],
    pub stable: bool,
    pub deprecated: bool,
    /// A `CodeDetail` expression.
    pub detail: TokenStream,
}

/// Emits a `::omni_codes::CodeInfo { .. }` expression. `codes` is the path to
/// the `omni_codes` crate as seen from the emitting crate.
pub fn code_info(codes: &TokenStream, c: &CodeInfoTokens) -> TokenStream {
    let CodeInfoTokens {
        code,
        summary,
        explanation,
        related,
        stable,
        deprecated,
        detail,
    } = c;
    quote! {
        #codes::CodeInfo {
            code: #code,
            summary: #summary,
            explanation: #explanation,
            related: &[#(#related),*],
            stable: #stable,
            deprecated: #deprecated,
            detail: #detail,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_detection() {
        assert!(!has_placeholder("plain"));
        assert!(!has_placeholder("literal {{brace}}"));
        assert!(has_placeholder("value {x}"));
        assert!(has_placeholder("{{ and {x}"));
    }

    #[test]
    fn docs_split() {
        let f: syn::ItemStruct = syn::parse_quote! {
            /// Summary line
            /// continues.
            ///
            /// Body one.
            /// Body two.
            struct X;
        };
        let (s, e) = doc_parts(&f.attrs);
        assert_eq!(s, "Summary line continues.");
        assert_eq!(e, "Body one.\nBody two.");
    }
}
