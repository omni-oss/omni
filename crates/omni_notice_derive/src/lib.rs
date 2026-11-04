use omni_derive_support::{
    CodeInfoTokens, code_info, doc_parts, has_placeholder, placeholder_names,
    valid_code,
};
use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{
    Data, DeriveInput, Expr, Fields, Ident, LitStr, parse_macro_input,
    spanned::Spanned,
};

/// Derives `Notice` (and the sealed `Code`), generates `Display` from
/// `message`, and registers the code in `omni_codes`.
///
/// ```ignore
/// /// Could not write a cache entry. The task result is unaffected.
/// #[derive(Notice)]
/// #[notice(code = "cache.write_skipped", level = error, stable,
///          message = "could not write cache entry for '{entry}'")]
/// struct CacheWriteSkipped { entry: String }
/// ```
#[proc_macro_derive(Notice, attributes(notice))]
pub fn derive_notice(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand(input) {
        Ok(t) => t.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

#[derive(Default)]
struct Attrs {
    code: Option<LitStr>,
    level: Option<Ident>,
    message: Option<LitStr>,
    essential: bool,
    stable: bool,
    deprecated: bool,
    related: Vec<LitStr>,
    explain: Option<Expr>,
}

fn parse_attrs(input: &DeriveInput) -> syn::Result<Attrs> {
    let mut a = Attrs::default();
    for attr in input.attrs.iter().filter(|a| a.path().is_ident("notice")) {
        attr.parse_nested_meta(|m| {
            let name = m.path.get_ident().map(|i| i.to_string());
            match name.as_deref() {
                Some("code") => a.code = Some(m.value()?.parse()?),
                Some("level") => a.level = Some(m.value()?.parse()?),
                Some("message") => a.message = Some(m.value()?.parse()?),
                Some("essential") => a.essential = true,
                Some("stable") => a.stable = true,
                Some("deprecated") => a.deprecated = true,
                Some("related") => a.related.push(m.value()?.parse()?),
                Some("explain") => a.explain = Some(m.value()?.parse()?),
                _ => return Err(m.error("unknown `notice` attribute")),
            }
            Ok(())
        })?;
    }
    Ok(a)
}

fn expand(input: DeriveInput) -> syn::Result<TokenStream2> {
    let name = input.ident.clone();
    let a = parse_attrs(&input)?;

    if !input.generics.params.is_empty() {
        return Err(syn::Error::new(
            input.generics.span(),
            "generic notice types cannot be registered",
        ));
    }

    let code = a.code.clone().ok_or_else(|| {
        syn::Error::new(name.span(), "missing `code = \"..\"` in `#[notice]`")
    })?;
    if !valid_code(&code.value()) {
        return Err(syn::Error::new(
            code.span(),
            "code must be lowercase `snake_case` segments separated by dots, at least two (for example `cache.write_skipped`)",
        ));
    }
    let level = a.level.clone().ok_or_else(|| {
        syn::Error::new(
            name.span(),
            "missing `level = info | warn | error` in `#[notice]`",
        )
    })?;
    let level_variant = match level.to_string().as_str() {
        "info" => format_ident!("Info"),
        "warn" => format_ident!("Warn"),
        "error" => format_ident!("Error"),
        _ => {
            return Err(syn::Error::new(
                level.span(),
                "expected `info`, `warn` or `error`",
            ));
        }
    };
    let message = a.message.clone().ok_or_else(|| {
        syn::Error::new(
            name.span(),
            "missing `message = \"..\"` in `#[notice]`",
        )
    })?;

    let (summary, explanation) = doc_parts(&input.attrs);
    if summary.is_empty() {
        return Err(syn::Error::new(
            name.span(),
            "a notice needs a doc comment: its first line is the summary shown by `omni explain`",
        ));
    }

    // Fields in `fields()`; a field marked `#[notice(skip)]` is only available
    // to `message` (for text that is derived from other state).
    let mut skipped: Vec<Ident> = Vec::new();
    let named: Vec<Ident> = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Unit => Vec::new(),
            Fields::Named(n) => {
                let mut out = Vec::new();
                for f in &n.named {
                    let Some(id) = f.ident.clone() else { continue };
                    let mut skip = false;
                    for attr in
                        f.attrs.iter().filter(|a| a.path().is_ident("notice"))
                    {
                        attr.parse_nested_meta(|m| {
                            if m.path.is_ident("skip") {
                                skip = true;
                                Ok(())
                            } else {
                                Err(m
                                    .error("only `skip` is allowed on a field"))
                            }
                        })?;
                    }
                    if skip {
                        skipped.push(id);
                    } else {
                        out.push(id);
                    }
                }
                out
            }
            Fields::Unnamed(u) => {
                return Err(syn::Error::new(
                    u.span(),
                    "notice fields must be named so they have `snake_case` keys",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new(
                name.span(),
                "`Notice` can only be derived for structs",
            ));
        }
    };

    if a.essential
        && (!named.is_empty()
            || !skipped.is_empty()
            || has_placeholder(&message.value()))
    {
        return Err(syn::Error::new(
            name.span(),
            "an `essential` notice must be static: no fields and no placeholders in `message`",
        ));
    }

    let info_ident =
        format_ident!("__{}_CODE_INFO", name.to_string().to_uppercase());
    let codes = quote!(::omni_notice::__private::omni_codes);
    let essential = a.essential;
    let explanation_tokens = match &a.explain {
        Some(e) => quote!(#e),
        None => {
            let e = explanation.as_str();
            quote!(#e)
        }
    };
    let info = code_info(
        &codes,
        &CodeInfoTokens {
            code: &code,
            summary: &summary,
            explanation: explanation_tokens,
            related: &a.related,
            stable: a.stable,
            deprecated: a.deprecated,
            detail: quote!(#codes::CodeDetail::Notice {
                level: #codes::NoticeLevel::#level_variant,
                essential: #essential,
            }),
        },
    );

    let field_inserts = named.iter().map(|f| {
        let key = f.to_string();
        quote! {
            __m.insert(
                ::std::string::String::from(#key),
                ::omni_notice::__private::to_value(&self.#f),
            );
        }
    });
    // Only fields the message names are passed: an unused named argument is a
    // compile error, and a field may exist only for `fields()`.
    let used = placeholder_names(&message.value());
    let fmt_args = named
        .iter()
        .chain(skipped.iter())
        .filter(|f| used.contains(&f.to_string()))
        .map(|f| quote!(#f = &self.#f));

    let err_check = quote! {
        const _: () = {
            trait __AmbiguousIfError<A> { const __X: () = (); }
            impl<T: ?::core::marker::Sized> __AmbiguousIfError<()> for T {}
            struct __Invalid;
            impl<T: ?::core::marker::Sized + ::std::error::Error>
                __AmbiguousIfError<__Invalid> for T {}
            let _ = <#name as __AmbiguousIfError<_>>::__X;
        };
    };

    Ok(quote! {
        static #info_ident: #codes::CodeInfo = #info;

        impl #codes::__private::Sealed for #name {}

        impl #codes::Code for #name {
            const CODES: &'static [#codes::CodeInfo] = &[#info];
            fn info(&self) -> &'static #codes::CodeInfo { &#info_ident }
        }

        impl ::omni_notice::Notice for #name {
            #[allow(unused_mut)]
            fn fields(&self) -> ::std::collections::BTreeMap<
                ::std::string::String,
                ::omni_notice::__private::serde_json::Value,
            > {
                let mut __m = ::std::collections::BTreeMap::new();
                #(#field_inserts)*
                __m
            }
        }

        impl ::core::fmt::Display for #name {
            fn fmt(&self, __f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                ::core::write!(__f, #message #(, #fmt_args)*)
            }
        }

        #codes::submit!(#codes::CodeRegistration {
            crate_name: ::core::env!("CARGO_PKG_NAME"),
            codes: <#name as #codes::Code>::CODES,
            prefixes: &[],
        });

        impl #name {
            #[doc(hidden)]
            #[allow(dead_code)]
            pub const __CODE_REGISTRATION: #codes::CodeRegistration =
                #codes::CodeRegistration {
                    crate_name: ::core::env!("CARGO_PKG_NAME"),
                    codes: <#name as #codes::Code>::CODES,
                    prefixes: &[],
                };
        }

        #err_check
    })
}
