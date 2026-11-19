use axum::{
    Json,
    extract::{Query, State},
    response::{IntoResponse as _, Response},
};
use axum_extra::routing::TypedPath;
use derive_new::new;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::{
    extractors::{Credential, TenantCode},
    hooks::{ArtifactOp, OutcomeKind},
    response::data::Data,
    routes::v1::artifacts::common::{
        CommonArtifactQuery, NamespaceParam, Placement, after_response,
        authorize, before_hook, list_namespace, validate_ownership,
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

    #[param()]
    /// `shared` (default) or `isolated`
    pub namespace: Option<NamespaceParam>,

    #[param()]
    /// Scope label for the isolated namespace
    pub scope: Option<String>,
}

#[derive(
    Deserialize, Serialize, Debug, Default, new, ToSchema, PartialEq, Eq,
)]
pub struct CacheItem {
    pub digest: String,
    pub size: u64,
    /// The scope segment, for entries of an isolated namespace
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
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
    let requested = query.namespace.unwrap_or(NamespaceParam::Shared);
    let caller = authorize!(
        state.provider,
        &credential,
        &tenant_code,
        &query,
        crate::security::Scopes::of(&[Scope::List]),
        |c: &crate::security::Caller| c.can(Scope::List),
    );
    let placement =
        match Placement::resolve(&caller, requested, query.scope.as_deref()) {
            Ok(p) => p,
            Err(r) => return *r,
        };

    validate_ownership!(state.provider, &tenant_code, &query);

    let hook_ctx = before_hook!(
        state,
        ArtifactOp::GetMany,
        &caller,
        &tenant_code,
        &query,
        Vec::new(),
        &placement,
    );

    let all_artifacts = list_namespace(
        state.storage_backend.as_ref(),
        &placement,
        &tenant_code,
        &CommonArtifactQuery {
            org: query.org.clone(),
            ws: query.ws.clone(),
            env: query.env.clone(),
        },
    )
    .await;

    let response = match all_artifacts {
        Ok(r) => Json(Data::new(r)).into_response(),
        Err(response) => *response,
    };

    after_response(&state, hook_ctx, OutcomeKind::Hit, response).await
}
