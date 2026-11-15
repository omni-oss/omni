use axum::{
    body::Body,
    extract::{Query, State},
    response::{IntoResponse as _, Response},
};
use axum_extra::{response::InternalServerError, routing::TypedPath};
use futures::StreamExt;
use http::StatusCode;
use omni_remote_cache_storage_core::{SaveOutcome, error::Error};
use serde::Deserialize;
use utoipa::IntoParams;

use crate::{
    extractors::{ApiKey, TenantCode},
    hooks::{ArtifactOp, OutcomeKind},
    routes::v1::artifacts::common::{
        after_response, before_hook, container, guard, require_valid_digest,
        validate_ownership,
    },
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
#[tracing::instrument(skip(state, body))]
pub async fn put_artifact(
    PutArtifactPath { digest }: PutArtifactPath,
    Query(query): Query<PutArtifactQuery>,
    TenantCode(tenant_code): TenantCode,
    State(state): State<ServiceState>,
    ApiKey(api_key): ApiKey,
    body: Body,
) -> Response {
    guard!(
        state.provider,
        &api_key,
        &tenant_code,
        &query,
        &["write:artifacts"],
    );

    require_valid_digest!(&digest);

    validate_ownership!(state.provider, &tenant_code, &query);

    let hook_ctx = before_hook!(
        state,
        ArtifactOp::Put,
        &api_key,
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
    let stream = body.into_data_stream().map(|r| r.map_err(Error::custom));

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
        Err(e) => e.into_response(),
    };

    after_response(&state, hook_ctx, OutcomeKind::Stored, response).await
}
