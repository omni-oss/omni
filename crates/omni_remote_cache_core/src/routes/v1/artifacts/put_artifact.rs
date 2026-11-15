use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use axum::{
    body::Body,
    extract::{Query, State},
    response::{IntoResponse as _, Response},
};
use axum_extra::{response::InternalServerError, routing::TypedPath};
use bytes::Bytes;
use futures::{Stream, StreamExt};
use http::{HeaderMap, StatusCode, header};
use omni_remote_cache_storage_core::{SaveOutcome, error::Error};
use serde::Deserialize;
use utoipa::IntoParams;

use crate::{
    extractors::{Credential, TenantCode},
    hooks::{ArtifactOp, OutcomeKind},
    routes::v1::artifacts::common::{
        after_response, authenticate, before_hook, container,
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
    ),
    request_body(content_type = "application/octet-stream", description = "Raw file streaming content", content = Vec<u8>),
    responses(
        (status = NO_CONTENT, description = "Stored"),
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
    // A chunked body (or a lying Content-Length) is bounded while streaming:
    // the stream fails past the limit, so the backend aborts and nothing is
    // committed.
    let exceeded = Arc::new(AtomicBool::new(false));
    let stream = limit_stream(
        body.into_data_stream().map(|r| r.map_err(Error::custom)),
        limit,
        exceeded.clone(),
    );

    let x = state
        .storage_backend
        .save_stream_if_absent(&container, &digest, Box::pin(stream))
        .await
        .map_err(InternalServerError);

    let response = match x {
        Ok(SaveOutcome::Stored) => StatusCode::NO_CONTENT.into_response(),
        // Immutable: an existing entry is never replaced. The client treats
        // this as "already stored", and the hook logs it as AlreadyExists.
        Ok(SaveOutcome::AlreadyExists) => StatusCode::CONFLICT.into_response(),
        Err(_) if exceeded.load(Ordering::SeqCst) => too_large(),
        Err(e) => e.into_response(),
    };

    after_response(&state, hook_ctx, OutcomeKind::Stored, response).await
}

fn declared_length(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(header::CONTENT_LENGTH)?
        .to_str()
        .ok()?
        .parse()
        .ok()
}

/// Passes chunks through until `limit` bytes have been seen. The chunk that
/// crosses it, and any later error, ends the stream with an error and sets
/// `exceeded` when the limit caused it.
fn limit_stream<S>(
    stream: S,
    limit: u64,
    exceeded: Arc<AtomicBool>,
) -> impl Stream<Item = Result<Bytes, Error>> + Send + 'static
where
    S: Stream<Item = Result<Bytes, Error>> + Send + 'static,
{
    stream.scan((0u64, false), move |(total, finished), item| {
        if *finished {
            return std::future::ready(None);
        }
        let next = match item {
            Ok(chunk) => {
                *total = total.saturating_add(chunk.len() as u64);
                if *total > limit {
                    exceeded.store(true, Ordering::SeqCst);
                    *finished = true;
                    Some(Err(Error::custom(eyre::eyre!(
                        "artifact exceeds the size limit"
                    ))))
                } else {
                    Some(Ok(chunk))
                }
            }
            Err(e) => {
                *finished = true;
                Some(Err(e))
            }
        };
        std::future::ready(next)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn run(chunks: &[&'static str], limit: u64) -> (Vec<bool>, bool) {
        let exceeded = Arc::new(AtomicBool::new(false));
        let stream = futures::stream::iter(
            chunks
                .iter()
                .map(|c| Ok::<_, Error>(Bytes::from_static(c.as_bytes())))
                .collect::<Vec<_>>(),
        );
        let outcomes = limit_stream(stream, limit, exceeded.clone())
            .map(|item| item.is_ok())
            .collect::<Vec<_>>()
            .await;
        (outcomes, exceeded.load(Ordering::SeqCst))
    }

    #[tokio::test]
    async fn a_stream_at_the_limit_passes() {
        assert_eq!(run(&["abc", "de"], 5).await, (vec![true, true], false));
        assert_eq!(run(&[], 0).await, (vec![], false));
    }

    #[tokio::test]
    async fn the_chunk_that_crosses_the_limit_fails_and_ends_the_stream() {
        let (outcomes, exceeded) = run(&["abc", "def", "ghi"], 5).await;
        assert_eq!(outcomes, vec![true, false], "nothing after the failure");
        assert!(exceeded);
    }

    #[tokio::test]
    async fn one_oversized_chunk_fails_immediately() {
        assert_eq!(run(&["abcdef"], 5).await, (vec![false], true));
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
}
