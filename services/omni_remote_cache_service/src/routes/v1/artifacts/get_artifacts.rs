use axum::{
    Json,
    extract::{Query, State},
    response::{IntoResponse as _, Response},
};
use axum_extra::{response::InternalServerError, routing::TypedPath};
use derive_new::new;
use omni_remote_cache_storage::RemoteCacheStorageBackend;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::{
    extractors::TenantCode,
    response::data::Data,
    routes::v1::artifacts::common::{container, get_validation_response},
    state::ServiceState,
};

#[derive(TypedPath, Deserialize, Debug)]
#[typed_path("/")]
pub struct GetArtifactsPath {}

#[derive(Deserialize, IntoParams, Debug)]
#[into_params(parameter_in = Query)]
pub struct GetArtifactsQuery {
    #[param()]
    pub org: String,

    #[param()]
    pub ws: String,

    #[param()]
    pub env: String,
}

#[derive(Deserialize, Serialize, Debug, Default, new, ToSchema)]
pub struct CacheItem {
    pub digest: String,
    pub size: u64,
}

#[utoipa::path(
    get,
    description = "List artifacts",
    path = "",
    params(
        ("x-omni-tenant" = String, Header, description = "Tenant code"),
        GetArtifactsQuery
    ),
    responses(
        (status = OK, description = "Success", body = Data<Vec<CacheItem>>),
        (status = BAD_REQUEST, description = "Bad request"),
        (status = INTERNAL_SERVER_ERROR, description = "Internal server error")
    )
)]
#[tracing::instrument(skip(state))]
pub async fn get_artifacts(
    _: GetArtifactsPath,
    Query(query): Query<GetArtifactsQuery>,
    State(state): State<ServiceState>,
    TenantCode(tenant_code): TenantCode,
) -> Response {
    let validate_svc = state.provider.validation_service();

    let result = validate_svc
        .validate_ownership(&tenant_code, &query.org, &query.ws, &query.env)
        .await
        .map_err(InternalServerError);

    match result {
        Ok(r) => {
            if let Some(response) = get_validation_response(
                r.violations(),
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
    let all_artifacts = state
        .storage_backend
        .list(Some(container.as_str()))
        .await
        .map_err(InternalServerError);

    match all_artifacts {
        Ok(r) => Json(
            r.iter()
                .map(|item| CacheItem {
                    digest: item.key().to_string(),
                    size: item.size().as_u64(),
                })
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(e) => e.into_response(),
    }
}
