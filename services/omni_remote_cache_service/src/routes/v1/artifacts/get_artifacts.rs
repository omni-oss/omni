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
    extractors::{ApiKey, TenantCode},
    response::data::Data,
    routes::v1::artifacts::common::{container, guard, validate_ownership},
    state::ServiceState,
};

#[derive(TypedPath, Deserialize, Debug)]
#[typed_path("/")]
pub struct GetArtifactsPath {}

#[derive(Deserialize, IntoParams, Debug)]
#[into_params(parameter_in = Query)]
pub struct GetArtifactsQuery {
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

#[derive(
    Deserialize, Serialize, Debug, Default, new, ToSchema, PartialEq, Eq,
)]
pub struct CacheItem {
    pub digest: String,
    pub size: u64,
}

#[utoipa::path(
    get,
    description = "List artifacts",
    path = "",
    params(
        ("X-OMNI-TENANT" = String, Header, description = "Tenant code"),
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
    ApiKey(api_key): ApiKey,
) -> Response {
    guard!(
        state.provider,
        &api_key,
        &tenant_code,
        &query,
        &["list:artifacts"],
    );

    validate_ownership!(state.provider, &tenant_code, &query);

    let container = container(&query.org, &query.ws, &query.env);
    let all_artifacts = state
        .storage_backend
        .list(Some(container.as_str()))
        .await
        .map_err(InternalServerError);

    match all_artifacts {
        Ok(r) => Json(Data::new(
            r.iter()
                .map(|item| CacheItem {
                    digest: item.key().to_string(),
                    size: item.size().as_u64(),
                })
                .collect::<Vec<_>>(),
        ))
        .into_response(),
        Err(e) => e.into_response(),
    }
}
