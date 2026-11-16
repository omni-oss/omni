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
        after_response_with, authenticate, before_hook, container,
        require_valid_digest, validate_ownership,
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
    let caller = authenticate!(
        state.provider,
        &credential,
        &tenant_code,
        &query,
        Scope::Write,
    );

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

    let hook_ctx = before_hook!(
        state,
        ArtifactOp::Put,
        &caller,
        &tenant_code,
        &query,
        vec![digest.clone()],
    );

    let container =
        match container(&tenant_code, &query.org, &query.ws, &query.env) {
            Ok(c) => c,
            Err(_) => {
                return (StatusCode::BAD_REQUEST, "invalid org, ws or env")
                    .into_response();
            }
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
            namespace: "shared".to_owned(),
            scope: None,
            signature: None,
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
