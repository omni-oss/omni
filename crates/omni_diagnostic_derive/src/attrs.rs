use proc_macro2::Span;
use syn::{Attribute, Ident, LitStr, Path, Token, spanned::Spanned};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AudienceAttr {
    User,
    Environment,
    Internal,
}

impl AudienceAttr {
    pub fn variant(self) -> &'static str {
        match self {
            Self::User => "User",
            Self::Environment => "Environment",
            Self::Internal => "Internal",
        }
    }
}

#[derive(Default)]
pub struct Attrs {
    pub audience: Option<(AudienceAttr, Span)>,
    pub code: Option<LitStr>,
    pub hints: Vec<LitStr>,
    pub hints_fn: Option<Path>,
    pub details: Option<Path>,
    pub body: Option<Path>,
    pub forward: Option<Span>,
    pub skip: Option<Span>,
    pub delegate: Option<Span>,
    pub no_register: Option<Span>,
    pub stable: bool,
    pub related: Vec<LitStr>,
}

fn set_once<T>(
    slot: &mut Option<T>,
    value: T,
    meta: &syn::meta::ParseNestedMeta,
) -> syn::Result<()> {
    if slot.is_some() {
        return Err(meta.error("duplicate `diagnostic` attribute"));
    }
    *slot = Some(value);
    Ok(())
}

pub fn parse_attrs(attrs: &[Attribute]) -> syn::Result<Attrs> {
    let mut a = Attrs::default();
    for attr in attrs.iter().filter(|a| a.path().is_ident("diagnostic")) {
        attr.parse_nested_meta(|m| {
            let name = m.path.get_ident().map(|i| i.to_string());
            match name.as_deref() {
                Some("audience") => {
                    let v: Ident = m.value()?.parse()?;
                    let au = match v.to_string().as_str() {
                        "user" => AudienceAttr::User,
                        "environment" => AudienceAttr::Environment,
                        "internal" => AudienceAttr::Internal,
                        _ => {
                            return Err(syn::Error::new(
                                v.span(),
                                "expected `user`, `environment` or `internal`",
                            ));
                        }
                    };
                    set_once(&mut a.audience, (au, v.span()), &m)
                }
                Some("code") => {
                    let v: LitStr = m.value()?.parse()?;
                    set_once(&mut a.code, v, &m)
                }
                Some("hint") => {
                    a.hints.push(m.value()?.parse()?);
                    Ok(())
                }
                Some("hints") => {
                    let v: Path = m.value()?.parse()?;
                    set_once(&mut a.hints_fn, v, &m)
                }
                Some("details") => {
                    let v: Path = m.value()?.parse()?;
                    set_once(&mut a.details, v, &m)
                }
                Some("body") => {
                    let v: Path = m.value()?.parse()?;
                    set_once(&mut a.body, v, &m)
                }
                Some("forward") => {
                    set_once(&mut a.forward, m.path.span(), &m)
                }
                Some("skip") => set_once(&mut a.skip, m.path.span(), &m),
                Some("delegate") => {
                    set_once(&mut a.delegate, m.path.span(), &m)
                }
                Some("related") => {
                    a.related.push(m.value()?.parse()?);
                    Ok(())
                }
                Some("stable") => {
                    a.stable = true;
                    Ok(())
                }
                Some("no_register") => {
                    set_once(&mut a.no_register, m.path.span(), &m)
                }
                Some("detail") => Err(m.error(
                    "`detail` belongs on a field; use `details = path` on a variant or struct",
                )),
                _ => Err(m.error("unknown `diagnostic` attribute")),
            }
        })?;
    }
    Ok(a)
}

/// Field level: `#[diagnostic(detail)]` or `#[diagnostic(detail = "key")]`.
/// Returns `Some(explicit_key)` wrapped in the span when present.
pub fn parse_field_attrs(
    attrs: &[Attribute],
) -> syn::Result<Option<(Option<String>, Span)>> {
    let mut out = None;
    for attr in attrs.iter().filter(|a| a.path().is_ident("diagnostic")) {
        attr.parse_nested_meta(|m| {
            if m.path.is_ident("detail") {
                if out.is_some() {
                    return Err(m.error("duplicate `detail`"));
                }
                let key = if m.input.peek(Token![=]) {
                    let v: LitStr = m.value()?.parse()?;
                    Some(v.value())
                } else {
                    None
                };
                out = Some((key, m.path.span()));
                Ok(())
            } else {
                Err(m.error("only `detail` is allowed on a field"))
            }
        })?;
    }
    Ok(out)
}

pub use omni_derive_support::valid_code;
