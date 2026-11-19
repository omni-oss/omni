use axum::{
    extract::FromRequestParts,
    response::{IntoResponse, Response},
};
use http::{StatusCode, header, request::Parts};

/// An opaque Bearer credential. Deliberately has no `Debug` implementation.
pub struct Credential(pub(crate) String);

pub struct CredentialRejection;

pub(crate) fn unauthorized_response() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Bearer")],
        "invalid or missing bearer credential",
    )
        .into_response()
}

impl IntoResponse for CredentialRejection {
    fn into_response(self) -> Response {
        unauthorized_response()
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Credential {
    type Rejection = CredentialRejection;

    async fn from_request_parts(
        parts: &mut Parts,
        _: &S,
    ) -> Result<Self, Self::Rejection> {
        let mut values = parts.headers.get_all(header::AUTHORIZATION).iter();
        let value = values.next().ok_or(CredentialRejection)?;
        if values.next().is_some() {
            return Err(CredentialRejection);
        }
        let value = value.to_str().map_err(|_| CredentialRejection)?;
        let (scheme, token) =
            value.split_once(' ').ok_or(CredentialRejection)?;
        let token = token.trim_start_matches(' ');
        // API keys are operator-chosen strings, so any visible ASCII token is
        // accepted (no spaces or control characters). It is only compared by value.
        if !scheme.eq_ignore_ascii_case("Bearer")
            || token.is_empty()
            || !token.bytes().all(|b| (0x21..=0x7e).contains(&b))
        {
            return Err(CredentialRejection);
        }
        Ok(Self(token.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::{HeaderValue, Request};

    async fn extract(
        values: &[&str],
    ) -> Result<Credential, CredentialRejection> {
        let (mut parts, _) = Request::new(()).into_parts();
        for value in values {
            parts.headers.append(
                header::AUTHORIZATION,
                HeaderValue::from_str(value).unwrap(),
            );
        }
        Credential::from_request_parts(&mut parts, &()).await
    }

    #[tokio::test]
    async fn bearer_is_case_insensitive_and_accepts_any_visible_ascii_token() {
        for value in [
            "Bearer token",
            "bEaReR   a-b.c_~+/==",
            "Bearer p@ss!w0rd:#%*=x",
            "Bearer a,b",
            "Bearer a=b",
            "Bearer =",
        ] {
            assert!(extract(&[value]).await.is_ok());
        }
        for values in [
            vec![],
            vec!["Basic token"],
            vec!["Bearer"],
            vec!["Bearer "],
            vec!["Bearer a b"],
            vec!["Bearer \u{e9}x"],
            vec!["Bearer token "],
            vec!["Bearer\ttoken"],
            vec!["Bearer a", "Bearer b"],
        ] {
            assert!(extract(&values).await.is_err(), "{values:?}");
        }
    }

    #[tokio::test]
    async fn legacy_header_is_not_an_authentication_fallback() {
        let (mut parts, _) = Request::new(()).into_parts();
        parts
            .headers
            .insert("x-api-key", HeaderValue::from_static("old-key"));
        assert!(
            Credential::from_request_parts(&mut parts, &())
                .await
                .is_err()
        );
        let response = CredentialRejection.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()[header::WWW_AUTHENTICATE], "Bearer");
    }
}
