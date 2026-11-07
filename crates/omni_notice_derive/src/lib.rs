use omni_derive_support::{
    CodeInfoTokens, code_info, code_registration, doc_parts, has_placeholder,
    placeholder_names, valid_code,
};
use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{
    Attribute, Data, DeriveInput, Expr, Fields, Ident, LitStr,
    parse_macro_input, spanned::Spanned,
};

/// Derives `Notice` (and the sealed `Code`), generates `Display` from
/// `message`, and registers each code in `omni_codes`.
/// Supports structs and enums with unit or named fields. Enum metadata and
/// doc summaries belong on each variant, not on the enclosing enum.
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
    docs: Option<Expr>,
}

/// Parses notice metadata from a struct or variant, rejecting unknown keys.
fn parse_attrs(attrs: &[Attribute]) -> syn::Result<Attrs> {
    let mut a = Attrs::default();
    for attr in attrs.iter().filter(|a| a.path().is_ident("notice")) {
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
                Some("docs") => a.docs = Some(m.value()?.parse()?),
                _ => return Err(m.error("unknown `notice` attribute")),
            }
            Ok(())
        })?;
    }
    Ok(a)
}

struct Case {
    pattern: TokenStream2,
    info: TokenStream2,
    code: LitStr,
    display: TokenStream2,
    fields: TokenStream2,
}

/// Validates one struct or enum variant and builds its metadata, field map
/// and display expressions with aliased field bindings.
fn build_case(
    name: &Ident,
    path: TokenStream2,
    fields: &Fields,
    attrs: &[Attribute],
) -> syn::Result<Case> {
    let a = parse_attrs(attrs)?;

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

    let (summary, explanation) = doc_parts(attrs);
    if summary.is_empty() {
        return Err(syn::Error::new(
            name.span(),
            "a notice needs a doc comment: its first line is the summary shown by `omni code explain`",
        ));
    }

    // Fields in `fields()`; a field marked `#[notice(skip)]` is only available
    // to `message` (for text that is derived from other state).
    let mut skipped: Vec<Ident> = Vec::new();
    let named: Vec<Ident> = match fields {
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
                            Err(m.error("only `skip` is allowed on a field"))
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

    let codes = quote!(::omni_notice::__private::omni_codes);
    let essential = a.essential;
    let explanation_tokens = match &a.docs {
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

    // Bind every field through an internal alias rather than its user name.
    // This prevents fields like `__m` and `__f` from shadowing generated locals.
    let bindings: Vec<_> = named
        .iter()
        .chain(&skipped)
        .enumerate()
        .map(|(i, f)| (f, format_ident!("__notice_field_{i}")))
        .collect();
    let pattern = match fields {
        Fields::Unit => path,
        Fields::Named(_) => {
            let fields = bindings
                .iter()
                .map(|(field, binding)| quote!(#field: #binding));
            quote!(#path { #(#fields),* })
        }
        Fields::Unnamed(_) => unreachable!(),
    };
    let field_inserts = bindings.iter().filter(|(f, _)| named.contains(f)).map(
        |(f, binding)| {
            let key = f.to_string();
            quote! {
                __m.insert(
                    ::std::string::String::from(#key),
                    ::omni_notice::__private::to_value(#binding),
                );
            }
        },
    );
    // Only fields the message names are passed: an unused named argument is a
    // compile error, and a field may exist only for `fields()`.
    let used = placeholder_names(&message.value());
    let fmt_args = bindings
        .iter()
        .filter(|(f, _)| used.contains(&f.to_string()))
        .map(|(f, binding)| quote!(#f = #binding));

    Ok(Case {
        pattern,
        info,
        code,
        display: quote!(::core::write!(__f, #message #(, #fmt_args)*)),
        fields: quote!({
            let mut __m = ::std::collections::BTreeMap::new();
            #(#field_inserts)*
            __m
        }),
    })
}

/// Normalizes the input into notice cases and emits the trait implementations
/// and one registration containing all codes. Rejects unsupported type forms,
/// enum-level metadata and duplicate variant codes.
fn expand(input: DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new(
            input.generics.span(),
            "generic notice types cannot be registered",
        ));
    }
    let cases = match &input.data {
        Data::Struct(s) => {
            vec![build_case(name, quote!(Self), &s.fields, &input.attrs)?]
        }
        Data::Enum(e) => {
            if let Some(attr) =
                input.attrs.iter().find(|a| a.path().is_ident("notice"))
            {
                return Err(syn::Error::new_spanned(
                    attr,
                    "`notice` attributes belong on each variant, not on the enum",
                ));
            }
            let mut cases = Vec::new();
            let mut seen = ::std::collections::HashSet::new();
            for v in &e.variants {
                let variant = &v.ident;
                let case = build_case(
                    variant,
                    quote!(Self::#variant),
                    &v.fields,
                    &v.attrs,
                )?;
                if !seen.insert(case.code.value()) {
                    return Err(syn::Error::new(
                        case.code.span(),
                        "duplicate notice code within this type",
                    ));
                }
                cases.push(case);
            }
            cases
        }
        Data::Union(u) => {
            return Err(syn::Error::new(
                u.union_token.span,
                "`Notice` cannot be derived for unions",
            ));
        }
    };
    let codes = quote!(::omni_notice::__private::omni_codes);
    let infos: Vec<_> = cases.iter().map(|c| &c.info).collect();
    let statics: Vec<_> = (0..cases.len())
        .map(|i| format_ident!("__NOTICE_INFO_{i}"))
        .collect();
    let arms = |bodies: Vec<TokenStream2>| {
        if cases.is_empty() {
            quote!(match *self {})
        } else {
            let arms = cases.iter().zip(bodies).map(|(case, body)| {
                let pat = &case.pattern;
                quote!(#pat => #body,)
            });
            quote!(match self { #(#arms)* })
        }
    };
    let info_body = arms(statics.iter().map(|s| quote!(&#s)).collect());
    let fields_body = arms(cases.iter().map(|c| c.fields.clone()).collect());
    let display_body = arms(cases.iter().map(|c| c.display.clone()).collect());
    let registration =
        code_registration(&codes, &quote!(<#name as #codes::Code>::CODES));

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
        const _: () = {
            #(static #statics: #codes::CodeInfo = #infos;)*

            impl #codes::__private::Sealed for #name {}

            impl #codes::Code for #name {
                const CODES: &'static [#codes::CodeInfo] = &[#(#infos),*];
                #[allow(unused_variables)]
                fn info(&self) -> &'static #codes::CodeInfo { #info_body }
            }

            impl ::omni_notice::Notice for #name {
                #[allow(unused_mut, unused_variables)]
                fn fields(&self) -> ::std::collections::BTreeMap<
                    ::std::string::String,
                    ::omni_notice::__private::serde_json::Value,
                > {
                    #fields_body
                }
            }

            impl ::core::fmt::Display for #name {
                #[allow(unused_variables)]
                fn fmt(&self, __f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    #display_body
                }
            }

            #codes::submit!(#registration);

            impl #name {
                #[doc(hidden)]
                #[allow(dead_code)]
                pub const __CODE_REGISTRATION: #codes::CodeRegistration =
                    #registration;
            }

            #err_check
        };
    })
}
