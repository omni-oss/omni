//! `#[derive(Diagnostic)]` for `omni_diagnostic`. See RFC 0021, section 4.7.

mod attrs;
mod expand;

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

#[proc_macro_derive(Diagnostic, attributes(diagnostic))]
pub fn derive_diagnostic(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand::expand(input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}
