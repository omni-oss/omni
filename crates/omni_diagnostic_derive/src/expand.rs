use std::collections::HashSet;

use omni_derive_support::{CodeInfoTokens, code_info, doc_parts};
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, Ident, LitStr, spanned::Spanned};

use crate::attrs::{
    Attrs, AudienceAttr, parse_attrs, parse_field_attrs, valid_code,
};

type Errs = Option<syn::Error>;

fn push(errs: &mut Errs, e: syn::Error) {
    match errs {
        Some(existing) => existing.combine(e),
        None => *errs = Some(e),
    }
}

struct Variant {
    idx: usize,
    pat: TokenStream,
    audience: AudienceAttr,
    forward: Option<Ident>,
    /// `skip`: the variant carries no diagnostic.
    skip: bool,
    /// `delegate`: look the diagnostic up in the field (an error or an
    /// `eyre::Report`).
    delegate: Option<Ident>,
    code: Option<LitStr>,
    hints: Vec<LitStr>,
    hints_fn: Option<syn::Path>,
    details_fn: Option<syn::Path>,
    detail_fields: Vec<(Ident, String)>,
    body_fn: Option<syn::Path>,
    stable: bool,
    related: Vec<LitStr>,
    docs: (String, String),
}

pub fn expand(input: DeriveInput) -> syn::Result<TokenStream> {
    let mut errs: Errs = None;
    let ty_attrs = parse_attrs(&input.attrs)?;
    let name = input.ident.clone();
    let no_register = ty_attrs.no_register;
    let type_audience = ty_attrs.audience.map(|(a, _)| a);
    let mut variants: Vec<Variant> = Vec::new();

    match &input.data {
        Data::Struct(s) => {
            let mut a = ty_attrs;
            a.no_register = None;
            if let Some(v) = build_variant(
                quote!(Self),
                &s.fields,
                a,
                None,
                name.span(),
                doc_parts(&input.attrs),
                &mut errs,
            )? {
                variants.push(v);
            }
        }
        Data::Enum(e) => {
            reject_on_enum(&ty_attrs, &mut errs);
            for v in &e.variants {
                let a = parse_attrs(&v.attrs)?;
                if let Some(span) = a.no_register {
                    push(
                        &mut errs,
                        syn::Error::new(
                            span,
                            "`no_register` belongs on the type, not a variant",
                        ),
                    );
                }
                let vi = &v.ident;
                if let Some(v) = build_variant(
                    quote!(Self::#vi),
                    &v.fields,
                    a,
                    type_audience,
                    vi.span(),
                    doc_parts(&v.attrs),
                    &mut errs,
                )? {
                    variants.push(v);
                }
            }
        }
        Data::Union(u) => {
            return Err(syn::Error::new(
                u.union_token.span,
                "`Diagnostic` cannot be derived for unions",
            ));
        }
    }

    let mut seen = HashSet::new();
    for v in &variants {
        if let Some(c) = &v.code
            && !seen.insert(c.value())
        {
            push(
                &mut errs,
                syn::Error::new(
                    c.span(),
                    "duplicate diagnostic code within this type",
                ),
            );
        }
    }

    let has_generics = !input.generics.params.is_empty();
    if no_register.is_none() && has_generics {
        push(
            &mut errs,
            syn::Error::new(
                input.generics.span(),
                "generic types cannot be registered; add `#[diagnostic(no_register)]` or use a concrete wrapper",
            ),
        );
    }

    if let Some(e) = errs {
        return Err(e);
    }

    for (i, v) in variants.iter_mut().enumerate() {
        v.idx = i;
    }

    // Callbacks are bound before any field pattern, so a field named like a
    // callback (`hints`, `body`, ...) cannot shadow it.
    let callbacks = variants.iter().map(|v| {
        let i = v.idx;
        let d = v.details_fn.as_ref().map(|p| {
            let id = format_ident!("__details_{i}");
            quote!(let #id: fn(&Self) -> ::omni_diagnostic::__private::serde_json::Value = #p;)
        });
        let h = v.hints_fn.as_ref().map(|p| {
            let id = format_ident!("__hints_{i}");
            quote!(let #id: fn(&Self) -> ::std::vec::Vec<::std::string::String> = #p;)
        });
        let b = v.body_fn.as_ref().map(|p| {
            let id = format_ident!("__body_{i}");
            quote!(let #id: fn(&Self, &mut dyn ::core::fmt::Write) -> ::core::fmt::Result = #p;)
        });
        quote!(#d #h #b)
    });
    let callbacks = quote!(#(#callbacks)*);

    let (ig, tg, wc) = input.generics.split_for_impl();
    let empty = variants.is_empty();

    let arms = |f: &dyn Fn(&Variant) -> TokenStream| -> TokenStream {
        if empty {
            return quote!(match *self {});
        }
        let arms = variants.iter().map(|v| {
            let pat = &v.pat;
            let body = f(v);
            quote!(#pat => #body,)
        });
        quote!(match self { #(#arms)* })
    };

    let code_body = arms(&|v| {
        if v.skip {
            quote!("")
        } else if let Some(d) = delegate_lookup(v) {
            quote!(#d.map(|__d| __d.code()).unwrap_or(""))
        } else {
            match (&v.forward, &v.code) {
                (Some(b), _) => {
                    quote!(::omni_diagnostic::Diagnostic::code(#b))
                }
                (None, Some(c)) => quote!(#c),
                (None, None) => unreachable!(),
            }
        }
    });
    let audience_body = arms(&|v| {
        if v.skip {
            quote!(::omni_diagnostic::Audience::Internal)
        } else if let Some(d) = delegate_lookup(v) {
            quote!(#d
                .map(|__d| __d.audience())
                .unwrap_or(::omni_diagnostic::Audience::Internal))
        } else {
            match &v.forward {
                Some(b) => {
                    quote!(::omni_diagnostic::Diagnostic::audience(#b))
                }
                None => {
                    let a = format_ident!("{}", v.audience.variant());
                    quote!(::omni_diagnostic::Audience::#a)
                }
            }
        }
    });
    let reportable_body = arms(&|v| {
        if v.skip {
            quote!(false)
        } else if let Some(d) = delegate_lookup(v) {
            quote!(#d.is_some())
        } else if let Some(b) = &v.forward {
            quote!(::omni_diagnostic::Diagnostic::reportable(#b))
        } else {
            quote!(true)
        }
    });
    let details_body = arms(&|v| {
        if v.skip {
            quote!(::core::option::Option::None)
        } else if let Some(d) = delegate_lookup(v) {
            quote!(#d.and_then(|__d| __d.details()))
        } else if let Some(b) = &v.forward {
            quote!(::omni_diagnostic::Diagnostic::details(#b))
        } else if v.details_fn.is_some() {
            let id = format_ident!("__details_{}", v.idx);
            quote!(::core::option::Option::Some(#id(self)))
        } else if v.detail_fields.is_empty() {
            quote!(::core::option::Option::None)
        } else {
            let ins = v.detail_fields.iter().map(|(bind, key)| {
                quote!(__m.insert(
                    ::std::string::ToString::to_string(#key),
                    ::omni_diagnostic::__private::to_value(#bind),
                );)
            });
            quote!({
                let mut __m = ::omni_diagnostic::__private::serde_json::Map::new();
                #(#ins)*
                ::core::option::Option::Some(
                    ::omni_diagnostic::__private::serde_json::Value::Object(__m),
                )
            })
        }
    });
    let hints_body = arms(&|v| {
        if v.skip {
            quote!(::std::vec::Vec::new())
        } else if let Some(d) = delegate_lookup(v) {
            quote!(#d.map(|__d| __d.hints()).unwrap_or_default())
        } else if let Some(b) = &v.forward {
            quote!(::omni_diagnostic::Diagnostic::hints(#b))
        } else {
            let hs = v
                .hints
                .iter()
                .map(|h| quote!(__h.push(::std::format!(#h));));
            let extra = v.hints_fn.as_ref().map(|_| {
                let id = format_ident!("__hints_{}", v.idx);
                quote!(__h.extend(#id(self));)
            });
            quote!({
                let mut __h = ::std::vec::Vec::<::std::string::String>::new();
                #extra
                #(#hs)*
                __h
            })
        }
    });
    let body_body = arms(&|v| {
        if v.skip {
            quote!(::core::result::Result::Ok(()))
        } else if let Some(d) = delegate_lookup(v) {
            quote!(match #d {
                ::core::option::Option::Some(__d) => __d.render_body(__out),
                ::core::option::Option::None => ::core::result::Result::Ok(()),
            })
        } else if let Some(b) = &v.forward {
            quote!(::omni_diagnostic::Diagnostic::render_body(#b, __out))
        } else if v.body_fn.is_some() {
            let id = format_ident!("__body_{}", v.idx);
            quote!(#id(self, __out))
        } else {
            quote!(::core::result::Result::Ok(()))
        }
    });

    // A type is "dynamic" when some variant has no code of its own, so
    // `Code::info` cannot be answered statically for the whole type.
    let has_forward = variants.iter().any(|v| v.code.is_none());
    let registration = if no_register.is_none() {
        let codes = variants.iter().filter_map(|v| {
            let c = v.code.as_ref()?;
            let a = format_ident!("{}", v.audience.variant());
            Some(quote!(::omni_diagnostic::CodeInfo {
                code: #c,
                audience: ::omni_diagnostic::Audience::#a,
            }))
        });
        let code_impl = code_impl(&name, &variants, has_forward);
        quote! {
            #code_impl

            impl #name {
                #[doc(hidden)]
                #[allow(dead_code)]
                pub const __DIAGNOSTIC_REGISTRATION: ::omni_diagnostic::Registration =
                    ::omni_diagnostic::Registration {
                        type_name: ::core::concat!(
                            ::core::module_path!(), "::", ::core::stringify!(#name)
                        ),
                        codes: &[#(#codes),*],
                        lookup: Self::__diagnostic_lookup,
                    };

                #[doc(hidden)]
                #[allow(dead_code)]
                pub fn __diagnostic_lookup<'a>(
                    e: &'a (dyn ::std::error::Error + 'static),
                ) -> ::core::option::Option<&'a dyn ::omni_diagnostic::Diagnostic> {
                    e.downcast_ref::<Self>()
                        .filter(|x| ::omni_diagnostic::Diagnostic::reportable(*x))
                        .map(|x| x as &dyn ::omni_diagnostic::Diagnostic)
                }
            }

            ::omni_diagnostic::register!(#name);
        }
    } else {
        quote!()
    };

    Ok(quote! {
        impl #ig ::omni_diagnostic::Diagnostic for #name #tg #wc {
            #[allow(unused_variables)]
            fn code(&self) -> &'static str { #code_body }

            #[allow(unused_variables)]
            fn audience(&self) -> ::omni_diagnostic::Audience { #audience_body }

            #[allow(unused_variables)]
            fn details(
                &self,
            ) -> ::core::option::Option<::omni_diagnostic::__private::serde_json::Value> {
                #callbacks
                #details_body
            }

            #[allow(unused_variables)]
            fn hints(&self) -> ::std::vec::Vec<::std::string::String> {
                #callbacks
                #hints_body
            }

            #[allow(unused_variables)]
            fn render_body(
                &self,
                __out: &mut dyn ::core::fmt::Write,
            ) -> ::core::fmt::Result {
                #callbacks
                #body_body
            }

            #[allow(unused_variables)]
            fn reportable(&self) -> bool { #reportable_body }
        }

        #registration
    })
}

fn reject_on_enum(a: &Attrs, errs: &mut Errs) {
    let msg = "only allowed on a variant (or on a struct), not on an enum";
    if let Some(c) = &a.code {
        push(errs, syn::Error::new(c.span(), msg));
    }
    if let Some(h) = a.hints.first() {
        push(errs, syn::Error::new(h.span(), msg));
    }
    if let Some(p) = &a.details {
        push(errs, syn::Error::new(p.span(), msg));
    }
    if let Some(p) = &a.body {
        push(errs, syn::Error::new(p.span(), msg));
    }
    if let Some(s) = a.forward {
        push(errs, syn::Error::new(s, msg));
    }
    for s in [a.skip, a.delegate].into_iter().flatten() {
        push(errs, syn::Error::new(s, msg));
    }
}

/// The expression yielding `Option<&dyn Diagnostic>` for a delegating variant.
fn delegate_lookup(v: &Variant) -> Option<TokenStream> {
    let bind = v.delegate.as_ref()?;
    Some(quote!(::omni_diagnostic::__delegate_lookup!(#bind)))
}

fn build_variant(
    path: TokenStream,
    fields: &Fields,
    attrs: Attrs,
    default_audience: Option<AudienceAttr>,
    name_span: Span,
    docs: (String, String),
    errs: &mut Errs,
) -> syn::Result<Option<Variant>> {
    let mut binds = Vec::new();
    let mut detail_fields = Vec::new();
    let mut detail_span = None;
    for (i, f) in fields.iter().enumerate() {
        let bind = match &f.ident {
            Some(id) => id.clone(),
            None => format_ident!("_{}", i),
        };
        if let Some((key, span)) = parse_field_attrs(&f.attrs)? {
            detail_span.get_or_insert(span);
            match (key, &f.ident) {
                (Some(k), _) => detail_fields.push((bind.clone(), k)),
                (None, Some(id)) => {
                    detail_fields.push((bind.clone(), id.to_string()))
                }
                (None, None) => push(
                    errs,
                    syn::Error::new(
                        span,
                        "tuple fields need an explicit key: `detail = \"key\"`",
                    ),
                ),
            }
        }
        binds.push(bind);
    }

    let pat = match fields {
        Fields::Named(_) => quote!(#path { #(#binds),* }),
        Fields::Unnamed(_) => quote!(#path ( #(#binds),* )),
        Fields::Unit => quote!(#path),
    };

    let audience = attrs
        .audience
        .map(|(a, _)| a)
        .or(default_audience)
        .unwrap_or(AudienceAttr::User);

    if attrs.skip.is_some() || attrs.delegate.is_some() {
        return non_owning_variant(
            pat,
            &binds,
            audience,
            &attrs,
            detail_span,
            docs,
            errs,
        );
    }

    if let Some(fspan) = attrs.forward {
        let mut conflict = |span: Option<Span>| {
            if let Some(s) = span {
                push(
                    errs,
                    syn::Error::new(s, "`forward` takes no other attributes"),
                );
            }
        };
        conflict(attrs.audience.map(|(_, s)| s));
        conflict(attrs.code.as_ref().map(|c| c.span()));
        conflict(attrs.hints.first().map(|h| h.span()));
        conflict(attrs.hints_fn.as_ref().map(|p| p.span()));
        conflict(attrs.details.as_ref().map(|p| p.span()));
        conflict(attrs.body.as_ref().map(|p| p.span()));
        conflict(detail_span);
        if binds.len() != 1 {
            push(
                errs,
                syn::Error::new(fspan, "`forward` requires exactly one field"),
            );
            return Ok(None);
        }
        return Ok(Some(Variant {
            idx: 0,
            pat,
            audience,
            forward: Some(binds[0].clone()),
            skip: false,
            delegate: None,
            code: None,
            hints: Vec::new(),
            hints_fn: None,
            details_fn: None,
            detail_fields: Vec::new(),
            body_fn: None,
            stable: false,
            related: Vec::new(),
            docs,
        }));
    }

    let code = match attrs.code {
        Some(c) => {
            if !valid_code(&c.value()) {
                push(
                    errs,
                    syn::Error::new(
                        c.span(),
                        "invalid code: expected dotted lowercase segments like `area.name` (`^[a-z][a-z0-9_]*(\\.[a-z][a-z0-9_]*)+$`)",
                    ),
                );
            }
            Some(c)
        }
        None => {
            push(
                errs,
                syn::Error::new(
                    name_span,
                    "missing `#[diagnostic(code = \"...\")]` (or `forward`)",
                ),
            );
            None
        }
    };

    if let (Some(p), Some(_)) = (&attrs.details, detail_span) {
        push(
            errs,
            syn::Error::new(
                p.span(),
                "`details = ...` cannot be combined with field `detail`",
            ),
        );
    }

    let hints = attrs
        .hints
        .iter()
        .map(|h| rewrite_hint(h, fields, errs))
        .collect();

    Ok(Some(Variant {
        idx: 0,
        pat,
        audience,
        forward: None,
        skip: false,
        delegate: None,
        code,
        hints,
        hints_fn: attrs.hints_fn,
        details_fn: attrs.details,
        detail_fields,
        body_fn: attrs.body,
        stable: attrs.stable,
        related: attrs.related,
        docs,
    }))
}

/// `skip` and `delegate`: variants with no code of their own. They take no
/// other attribute.
fn non_owning_variant(
    pat: TokenStream,
    binds: &[Ident],
    audience: AudienceAttr,
    attrs: &Attrs,
    detail_span: Option<Span>,
    docs: (String, String),
    errs: &mut Errs,
) -> syn::Result<Option<Variant>> {
    let chosen: Vec<Span> = [attrs.skip, attrs.delegate, attrs.forward]
        .into_iter()
        .flatten()
        .collect();
    if chosen.len() > 1 {
        push(
            errs,
            syn::Error::new(
                chosen[1],
                "use only one of `skip`, `delegate` and `forward`",
            ),
        );
        return Ok(None);
    }
    let mut conflict = |span: Option<Span>| {
        if let Some(s) = span {
            push(
                errs,
                syn::Error::new(
                    s,
                    "`skip` and `delegate` take no other attributes",
                ),
            );
        }
    };
    conflict(attrs.audience.map(|(_, s)| s));
    conflict(attrs.code.as_ref().map(|c| c.span()));
    conflict(attrs.hints.first().map(|h| h.span()));
    conflict(attrs.hints_fn.as_ref().map(|p| p.span()));
    conflict(attrs.details.as_ref().map(|p| p.span()));
    conflict(attrs.body.as_ref().map(|p| p.span()));
    conflict(detail_span);

    let delegate = match attrs.delegate {
        Some(span) => {
            if binds.len() != 1 {
                push(
                    errs,
                    syn::Error::new(
                        span,
                        "`delegate` requires exactly one field",
                    ),
                );
                return Ok(None);
            }
            Some(binds[0].clone())
        }
        None => None,
    };
    Ok(Some(Variant {
        idx: 0,
        pat,
        audience,
        forward: None,
        skip: attrs.skip.is_some(),
        delegate,
        code: None,
        hints: Vec::new(),
        hints_fn: None,
        details_fn: None,
        detail_fields: Vec::new(),
        body_fn: None,
        stable: false,
        related: Vec::new(),
        docs,
    }))
}

/// Rewrites `{0}` to `{_0}` (our tuple bindings) and checks every referenced
/// field exists on the variant.
fn rewrite_hint(lit: &LitStr, fields: &Fields, errs: &mut Errs) -> LitStr {
    let s = lit.value();
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '{' && chars.get(i + 1) == Some(&'{') {
            out.push_str("{{");
            i += 2;
        } else if c == '}' && chars.get(i + 1) == Some(&'}') {
            out.push_str("}}");
            i += 2;
        } else if c == '{' {
            let Some(end) = chars[i..].iter().position(|c| *c == '}') else {
                out.extend(&chars[i..]);
                break;
            };
            let inner: String = chars[i + 1..i + end].iter().collect();
            let (arg, spec) = match inner.find(':') {
                Some(p) => (&inner[..p], &inner[p..]),
                None => (inner.as_str(), ""),
            };
            if arg.is_empty() {
                push(
                    errs,
                    syn::Error::new(
                        lit.span(),
                        "implicit `{}` is not supported in hints; name the field",
                    ),
                );
            } else if arg.chars().all(|c| c.is_ascii_digit()) {
                let n: usize = arg.parse().unwrap_or(usize::MAX);
                if !matches!(fields, Fields::Unnamed(_)) || n >= fields.len() {
                    push(
                        errs,
                        syn::Error::new(
                            lit.span(),
                            format!(
                                "hint references field `{arg}`, which does not exist"
                            ),
                        ),
                    );
                }
                out.push_str(&format!("{{_{arg}{spec}}}"));
            } else {
                let exists = fields
                    .iter()
                    .any(|f| f.ident.as_ref().is_some_and(|id| id == arg));
                if !exists {
                    push(
                        errs,
                        syn::Error::new(
                            lit.span(),
                            format!(
                                "hint references field `{arg}`, which does not exist"
                            ),
                        ),
                    );
                }
                out.push_str(&format!("{{{arg}{spec}}}"));
            }
            i += end + 1;
        } else {
            out.push(c);
            i += 1;
        }
    }
    LitStr::new(&out, lit.span())
}

/// `impl Code`, the sealed marker and the `omni_codes` submission for a type
/// whose variants all carry their own code.
fn code_impl(
    name: &Ident,
    variants: &[Variant],
    has_forward: bool,
) -> TokenStream {
    let codes = quote!(::omni_diagnostic::__private::omni_codes);
    let infos: Vec<TokenStream> = variants
        .iter()
        .filter_map(|v| {
            let c = v.code.as_ref()?;
            let a = format_ident!("{}", v.audience.variant());
            let (summary, explanation) = &v.docs;
            Some(code_info(
                &codes,
                &CodeInfoTokens {
                    code: c,
                    summary,
                    explanation: quote!(#explanation),
                    related: &v.related,
                    stable: v.stable,
                    deprecated: false,
                    detail: quote!(#codes::CodeDetail::Error {
                        audience: #codes::Audience::#a,
                    }),
                },
            ))
        })
        .collect();
    let statics: Vec<Ident> = (0..infos.len())
        .map(|i| format_ident!("__CODE_INFO_{}", i))
        .collect();
    let arms = variants
        .iter()
        .filter(|v| v.code.is_some())
        .enumerate()
        .map(|(i, v)| {
            let pat = &v.pat;
            let s = &statics[i];
            quote!(#pat => &#s,)
        });
    let empty = infos.is_empty();
    let body = if empty {
        quote!(match *self {})
    } else {
        quote!(match self { #(#arms)* })
    };
    let submit = quote! {
        #codes::submit!(#codes::CodeRegistration {
            crate_name: ::core::env!("CARGO_PKG_NAME"),
            codes: &[#(#infos),*],
            prefixes: &[],
        });
    };
    // A forwarding variant has no code of its own, so `Code::info` cannot be
    // answered statically for the type; its own codes are still registered.
    if has_forward {
        return quote!(const _: () = { #submit };);
    }
    quote! {
        const _: () = {
            #(static #statics: #codes::CodeInfo = #infos;)*

            impl #codes::__private::Sealed for #name {}

            impl #codes::Code for #name {
                const CODES: &'static [#codes::CodeInfo] = &[#(#infos),*];
                #[allow(unused_variables)]
                fn info(&self) -> &'static #codes::CodeInfo { #body }
            }

            #submit
        };
    }
}
