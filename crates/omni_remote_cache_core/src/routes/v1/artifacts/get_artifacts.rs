use axum::{
    Json,
    extract::{Query, State},
    response::{IntoResponse as _, Response},
};
use axum_extra::{response::InternalServerError, routing::TypedPath};
use derive_new::new;
use http::StatusCode;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::{
    extractors::{Credential, TenantCode},
    hooks::{ArtifactOp, OutcomeKind},
    response::data::Data,
    routes::v1::artifacts::common::{
        after_response, authenticate, before_hook, container,
        validate_ownership,
    },
    security::Scope,
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
#[tracing::instrument(skip_all, fields(org = %query.org, ws = %query.ws, env = %query.env))]
pub async fn get_artifacts(
    _: GetArtifactsPath,
    Query(query): Query<GetArtifactsQuery>,
    State(state): State<ServiceState>,
    TenantCode(tenant_code): TenantCode,
    Credential(credential): Credential,
) -> Response {
    let caller = authenticate!(
        state.provider,
        &credential,
        &tenant_code,
        &query,
        Scope::List,
    );

    validate_ownership!(state.provider, &tenant_code, &query);

    let hook_ctx = before_hook!(
        state,
        ArtifactOp::GetMany,
        &caller,
        &tenant_code,
        &query,
        Vec::new(),
    );

    let container =
        match container(&tenant_code, &query.org, &query.ws, &query.env) {
            Ok(c) => c,
            Err(_) => {
                return (StatusCode::BAD_REQUEST, "invalid org, ws or env")
                    .into_response();
            }
        };
    let all_artifacts = state
        .storage_backend
        .list(&container)
        .await
        .map_err(InternalServerError);

    let response = match all_artifacts {
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
    };

    after_response(&state, hook_ctx, OutcomeKind::Hit, response).await
}
