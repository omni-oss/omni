use axum::{
    body::Body,
    extract::{Query, State},
    response::{IntoResponse as _, Response},
};
use axum_extra::{response::InternalServerError, routing::TypedPath};
use futures::StreamExt;
use http::{HeaderMap, StatusCode, header};
use omni_remote_cache_storage_core::{SaveOutcome, error::Error};
use serde::Deserialize;
use time::OffsetDateTime;
use utoipa::IntoParams;

use crate::{
    entry::{SealFailure, SealParams, TrailerCredential, is_sha256_hex, seal},
    extractors::{Credential, TenantCode},
    hooks::{ArtifactOp, OutcomeKind},
    routes::v1::artifacts::common::{
        CommonArtifactQuery, NamespaceParam, Placement, after_response_with,
        authorize, before_hook, require_valid_digest, validate_ownership,
    },
    security::Scope,
    state::ServiceState,
};

#[derive(TypedPath, Deserialize, Debug, IntoParams)]
#[typed_path("/{digest}")]
pub struct PutArtifactPath {
    #[param()]
    /// The digest of the artifact
    pub digest: String,
}

#[derive(Deserialize, IntoParams, Debug)]
#[into_params(parameter_in = Query)]
pub struct PutArtifactQuery {
    #[param()]
    /// The organization code
    pub org: String,
    #[param()]
    /// The workspace code
    pub ws: String,
    #[param()]
    /// The environment code
    pub env: String,
    #[param()]
    /// `isolated` (default) or `shared` (needs `publish:artifacts`)
    pub target: Option<NamespaceParam>,
    #[param()]
    /// Scope label for the isolated namespace
    pub scope: Option<String>,
}

#[utoipa::path(
    put,
    description = "Upload an artifact",
    path = "/{digest}",
    params(
        PutArtifactPath,
        PutArtifactQuery,
        ("X-OMNI-TENANT" = String, Header, description = "Tenant code"),
        ("X-Omni-Content-Sha256" = String, Header, description = "Lowercase hex SHA-256 of the artifact. Required, exactly once. The upload is rejected and nothing is stored when the bytes do not match"),
    ),
    request_body(content_type = "application/octet-stream", description = "Raw file streaming content", content = Vec<u8>),
    responses(
        (status = NO_CONTENT, description = "Stored"),
        (status = PAYLOAD_TOO_LARGE, description = "The artifact exceeds the maximum size"),
        (status = UNPROCESSABLE_ENTITY, description = "The artifact does not match X-Omni-Content-Sha256"),
        (status = CONFLICT, description = "An entry already exists; entries are immutable"),
        (status = BAD_REQUEST, description = "Bad request"),
        (status = INTERNAL_SERVER_ERROR, description = "Internal server error")
    )
)]
#[tracing::instrument(skip_all, fields(digest = %digest, org = %query.org, ws = %query.ws, env = %query.env))]
pub async fn put_artifact(
    PutArtifactPath { digest }: PutArtifactPath,
    Query(query): Query<PutArtifactQuery>,
    TenantCode(tenant_code): TenantCode,
    State(state): State<ServiceState>,
    Credential(credential): Credential,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let requested = query.target.unwrap_or(NamespaceParam::Isolated);
    let caller = match requested {
        NamespaceParam::Shared => authorize!(
            state.provider,
            &credential,
            &tenant_code,
            &query,
            crate::security::Scopes::of(&[Scope::Publish]),
            |c: &crate::security::Caller| c.can_publish(),
        ),
        NamespaceParam::Isolated => authorize!(
            state.provider,
            &credential,
            &tenant_code,
            &query,
            crate::security::Scopes::of(&[Scope::Write, Scope::Publish]),
            |c: &crate::security::Caller| c.can_write_isolated(),
        ),
    };
    let placement =
        match Placement::resolve(&caller, requested, query.scope.as_deref()) {
            Ok(p) => p,
            Err(r) => return *r,
        };

    require_valid_digest!(&digest);

    validate_ownership!(state.provider, &tenant_code, &query);

    let limit = state.max_artifact_size;
    let too_large = || {
        (
            StatusCode::PAYLOAD_TOO_LARGE,
            format!("artifact exceeds the maximum size of {limit} bytes"),
        )
            .into_response()
    };
    // Reject an honest oversized upload before reading any of it. Like a
    // malformed digest, this is decided before the hooks run, so a rejected
    // request leaves no `before` without an `after`. An oversized upload is
    // refused whether or not the key already holds an entry (413, not 409).
    if declared_length(&headers).is_some_and(|length| length > limit) {
        return too_large();
    }

    // Decided before the hooks and before reading the body, like the size above.
    let declared_sha256 = match declared_sha256(&headers) {
        Ok(hash) => hash,
        Err(message) => {
            return (StatusCode::BAD_REQUEST, message).into_response();
        }
    };

    // Only a shared entry is signed (RFC 0030, section 4.5), and the server
    // never verifies: it stores a well formed string for readers to check.
    let signature = match declared_signature(&headers) {
        Ok(None) => None,
        Ok(Some(_)) if placement.trailer_namespace() != "shared" => {
            return (
                StatusCode::BAD_REQUEST,
                "only a shared entry can carry X-Omni-Signature",
            )
                .into_response();
        }
        Ok(Some(signature)) => Some(signature),
        Err(message) => {
            return (StatusCode::BAD_REQUEST, message).into_response();
        }
    };

    let hook_ctx = before_hook!(
        state,
        ArtifactOp::Put,
        &caller,
        &tenant_code,
        &query,
        vec![digest.clone()],
        &placement,
    );

    let container = match placement.container(
        &tenant_code,
        &CommonArtifactQuery {
            org: query.org.clone(),
            ws: query.ws.clone(),
            env: query.env.clone(),
        },
    ) {
        Ok(c) => c,
        Err(r) => return *r,
    };
    // The server seals the entry: it hashes the body as it streams, stops at the
    // size limit (a chunked body, or one that lies about its length, is bounded
    // here), compares with the declared hash at the end and only then appends
    // the provenance trailer. Any failure ends the stream with an error, so the
    // backend aborts and nothing is committed.
    let (stream, sealed) = seal(
        Box::pin(body.into_data_stream().map(|r| r.map_err(Error::custom))),
        SealParams {
            declared_sha256: declared_sha256.clone(),
            limit,
            writer_credential: TrailerCredential {
                kind: caller.credential().kind().as_str().to_owned(),
                id: caller.credential().id().to_owned(),
            },
            writer_principal: caller.principal().to_owned(),
            namespace: placement.trailer_namespace().to_owned(),
            scope: placement.scope(),
            signature,
            now: OffsetDateTime::now_utc,
        },
    );

    let x = state
        .storage_backend
        .save_stream_if_absent(&container, &digest, stream)
        .await
        .map_err(InternalServerError);

    let response = match x {
        Ok(SaveOutcome::Stored) => StatusCode::NO_CONTENT.into_response(),
        // Immutable: an existing entry is never replaced. The client treats
        // this as "already stored", and the hook logs it as AlreadyExists.
        Ok(SaveOutcome::AlreadyExists) => StatusCode::CONFLICT.into_response(),
        // The handle is read after the backend returned. `None` means the
        // stream did not fail, so the backend did.
        Err(e) => match sealed.failure() {
            Some(SealFailure::TooLarge) => too_large(),
            Some(SealFailure::Mismatch) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "the artifact does not match X-Omni-Content-Sha256",
            )
                .into_response(),
            _ => e.into_response(),
        },
    };

    after_response_with(
        &state,
        hook_ctx,
        OutcomeKind::Stored,
        response,
        Some(declared_sha256),
    )
    .await
}

fn declared_length(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(header::CONTENT_LENGTH)?
        .to_str()
        .ok()?
        .parse()
        .ok()
}

/// An optional `X-Omni-Signature`: at most one header, `<16 lowercase hex key
/// id>:<88 characters of padded base64>`, so whatever is stored reads back.
fn declared_signature(
    headers: &HeaderMap,
) -> Result<Option<String>, &'static str> {
    let mut values = headers.get_all("x-omni-signature").iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err("send at most one X-Omni-Signature header");
    }
    match value.to_str() {
        Ok(text) if is_well_formed_signature(text) => Ok(Some(text.to_owned())),
        _ => Err(
            "X-Omni-Signature must be <16 hex key id>:<88 base64 characters>",
        ),
    }
}

/// True for `<16 lowercase hex>:<base64 of exactly 64 bytes>`, the shape of an
/// Ed25519 signature. The bytes are only decoded, never verified here.
fn is_well_formed_signature(text: &str) -> bool {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let Some((id, encoded)) = text.split_once(':') else {
        return false;
    };
    id.len() == 16
        && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        && encoded.len() == 88
        && STANDARD
            .decode(encoded)
            .is_ok_and(|bytes| bytes.len() == 64)
}

/// The client's SHA-256 of the artifact: exactly one `X-Omni-Content-Sha256`
/// header holding 64 lowercase hex characters.
fn declared_sha256(headers: &HeaderMap) -> Result<String, &'static str> {
    const NAME: &str = "x-omni-content-sha256";
    let mut values = headers.get_all(NAME).iter();
    let (Some(value), None) = (values.next(), values.next()) else {
        return Err(
            "send exactly one X-Omni-Content-Sha256 header (lowercase hex SHA-256 of the artifact)",
        );
    };
    match value.to_str() {
        Ok(text) if is_sha256_hex(text) => Ok(text.to_owned()),
        _ => Err(
            "X-Omni-Content-Sha256 must be 64 lowercase hexadecimal characters",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_signature_grammar_is_strict_and_optional() {
        let sig = format!("{}:{}==", "0123456789abcdef", "A".repeat(86));
        let one = |value: &str| {
            let mut headers = HeaderMap::new();
            headers.insert("x-omni-signature", value.parse().unwrap());
            declared_signature(&headers)
        };
        assert_eq!(declared_signature(&HeaderMap::new()), Ok(None));
        assert_eq!(one(&sig), Ok(Some(sig.clone())));
        for bad in [
            sig.to_uppercase(),
            sig.replace(':', ";"),
            // Valid base64 of 65 bytes: not an Ed25519 signature.
            format!("{}:{}=", "0123456789abcdef", "A".repeat(87)),
            format!("{}:{}=", "0123456789abcde", "A".repeat(86)),
            format!("{}:{}", "0123456789abcdef", "A".repeat(88)),
            format!("{}:{}==", "0123456789abcdef", "A".repeat(86))
                .replace("==", "==="),
            format!("{}:{}=", "0123456789abcdef", "-".repeat(87)),
            format!("{sig}A"),
            String::new(),
        ] {
            assert!(one(&bad).is_err(), "{bad:?}");
        }
        let mut twice = HeaderMap::new();
        twice.append("x-omni-signature", sig.parse().unwrap());
        twice.append("x-omni-signature", sig.parse().unwrap());
        assert!(declared_signature(&twice).is_err());
    }

    #[test]
    fn content_length_is_read_only_when_it_is_a_number() {
        let mut headers = HeaderMap::new();
        assert_eq!(declared_length(&headers), None);
        headers.insert(header::CONTENT_LENGTH, "12".parse().unwrap());
        assert_eq!(declared_length(&headers), Some(12));
        headers.insert(header::CONTENT_LENGTH, "x".parse().unwrap());
        assert_eq!(declared_length(&headers), None);
    }

    #[test]
    fn the_declared_hash_is_one_header_of_lowercase_hex() {
        let good = "ab".repeat(32);
        let with = |values: &[&str]| {
            let mut headers = HeaderMap::new();
            for value in values {
                headers.append("x-omni-content-sha256", value.parse().unwrap());
            }
            declared_sha256(&headers)
        };
        assert_eq!(with(&[&good]), Ok(good.clone()));
        let upper = "AB".repeat(32);
        let long = format!("{good}0");
        let spaced = format!(" {}", &good[1..]);
        let not_hex = "zz".repeat(32);
        for bad in [
            vec![],
            vec![good.as_str(), good.as_str()],
            vec![&good[..63]],
            vec![""],
            vec![upper.as_str()],
            vec![long.as_str()],
            vec![spaced.as_str()],
            vec![not_hex.as_str()],
        ] {
            assert!(with(&bad).is_err(), "{bad:?}");
        }
    }
}
