use std::collections::HashSet;

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
    pat: TokenStream,
    audience: AudienceAttr,
    forward: Option<Ident>,
    code: Option<LitStr>,
    hints: Vec<LitStr>,
    details_fn: Option<syn::Path>,
    detail_fields: Vec<(Ident, String)>,
    body_fn: Option<syn::Path>,
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

    let code_body = arms(&|v| match (&v.forward, &v.code) {
        (Some(b), _) => quote!(::omni_diagnostic::Diagnostic::code(#b)),
        (None, Some(c)) => quote!(#c),
        (None, None) => unreachable!(),
    });
    let audience_body = arms(&|v| match &v.forward {
        Some(b) => quote!(::omni_diagnostic::Diagnostic::audience(#b)),
        None => {
            let a = format_ident!("{}", v.audience.variant());
            quote!(::omni_diagnostic::Audience::#a)
        }
    });
    let details_body = arms(&|v| {
        if let Some(b) = &v.forward {
            quote!(::omni_diagnostic::Diagnostic::details(#b))
        } else if let Some(p) = &v.details_fn {
            quote!(::core::option::Option::Some(#p(self)))
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
        if let Some(b) = &v.forward {
            quote!(::omni_diagnostic::Diagnostic::hints(#b))
        } else {
            let hs = v
                .hints
                .iter()
                .map(|h| quote!(__h.push(::std::format!(#h));));
            quote!({
                let mut __h = ::std::vec::Vec::<::std::string::String>::new();
                #(#hs)*
                __h
            })
        }
    });
    let body_body = arms(&|v| {
        if let Some(b) = &v.forward {
            quote!(::omni_diagnostic::Diagnostic::render_body(#b, out))
        } else if let Some(p) = &v.body_fn {
            quote!(#p(self, out))
        } else {
            quote!(::core::result::Result::Ok(()))
        }
    });

    let registration = if no_register.is_none() {
        let codes = variants.iter().filter_map(|v| {
            let c = v.code.as_ref()?;
            let a = format_ident!("{}", v.audience.variant());
            Some(quote!(::omni_diagnostic::CodeInfo {
                code: #c,
                audience: ::omni_diagnostic::Audience::#a,
            }))
        });
        quote! {
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
                #details_body
            }

            #[allow(unused_variables)]
            fn hints(&self) -> ::std::vec::Vec<::std::string::String> { #hints_body }

            #[allow(unused_variables)]
            fn render_body(
                &self,
                out: &mut dyn ::core::fmt::Write,
            ) -> ::core::fmt::Result {
                #body_body
            }
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
}

fn build_variant(
    path: TokenStream,
    fields: &Fields,
    attrs: Attrs,
    default_audience: Option<AudienceAttr>,
    name_span: Span,
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
            pat,
            audience,
            forward: Some(binds[0].clone()),
            code: None,
            hints: Vec::new(),
            details_fn: None,
            detail_fields: Vec::new(),
            body_fn: None,
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
        pat,
        audience,
        forward: None,
        code,
        hints,
        details_fn: attrs.details,
        detail_fields,
        body_fn: attrs.body,
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
