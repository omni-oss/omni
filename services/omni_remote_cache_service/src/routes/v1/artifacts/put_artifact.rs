use axum::{
    body::Body,
    extract::{Query, State},
    response::{IntoResponse as _, Response},
};
use axum_extra::{response::InternalServerError, routing::TypedPath};
use http::StatusCode;
use omni_remote_cache_storage::RemoteCacheStorageBackend;
use serde::Deserialize;
use tokio_stream::StreamExt;
use utoipa::IntoParams;

use crate::{
    extractors::{ApiKey, TenantCode},
    routes::v1::artifacts::common::{
        container, get_validation_response, guard,
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
        (status = NO_CONTENT, description = "Success"),
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

    let validate_svc = state.provider.validation_service();

    let result = validate_svc
        .validate_ownership(&tenant_code, &query.org, &query.ws, &query.env)
        .await
        .map_err(InternalServerError);

    match result {
        Ok(r) => {
            if let Some(response) = get_validation_response(
                r.violations(),
                &tenant_code,
                &query.org,
                &query.ws,
                &query.env,
            ) {
                return response;
            }
        }
        Err(e) => return e.into_response(),
    }

    let container = container(&query.org, &query.ws, &query.env);
    let stream = body.into_data_stream().filter_map(|r| match r {
        Ok(b) => Some(b),
        Err(e) => {
            trace::error!("Error reading from stream: {}", e);
            None
        }
    });

    let x = state
        .storage_backend
        .save_stream(Some(container.as_ref()), &digest, Box::pin(stream))
        .await
        .map_err(|e| InternalServerError(e));

    match x {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => e.into_response(),
    }
}
