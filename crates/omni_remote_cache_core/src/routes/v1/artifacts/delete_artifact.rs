use axum::{
    debug_handler,
    extract::{Query, State},
    response::{IntoResponse as _, Response},
};
use axum_extra::{response::InternalServerError, routing::TypedPath};
use http::StatusCode;
use serde::Deserialize;
use utoipa::IntoParams;

use crate::{
    extractors::{Credential, TenantCode},
    hooks::{ArtifactOp, OutcomeKind},
    routes::v1::artifacts::common::{
        CommonArtifactQuery, NamespaceParam, Placement, after_response,
        authorize, before_hook, require_valid_digest, validate_ownership,
    },
    security::Scope,
    state::ServiceState,
};

#[derive(TypedPath, Deserialize, Debug)]
#[typed_path("/{digest}")]
pub struct DeleteArtifactPath {
    pub digest: String,
}

#[derive(Deserialize, IntoParams, Debug)]
#[into_params(parameter_in = Query)]
pub struct DeleteArtifactQuery {
    /// The organization code
    pub org: String,
    /// The workspace code
    pub ws: String,
    /// The environment code
    pub env: String,
    /// `shared` (default) or `isolated`
    pub namespace: Option<NamespaceParam>,
    /// Scope label for the isolated namespace
    pub scope: Option<String>,
}

#[utoipa::path(
    delete,
    description = "Delete an artifact",
    path = "/{digest}",
    params(
        ("digest" = String, Path, description = "Artifact digest"),
        ("X-OMNI-TENANT" = String, Header, description = "Tenant code"),
        DeleteArtifactQuery
    ),
    responses(
        (status = NO_CONTENT, description = "Success"),
        (status = NOT_FOUND, description = "Not found"),
        (status = BAD_REQUEST, description = "Bad request"),
        (status = INTERNAL_SERVER_ERROR, description = "Internal server error")
    )
)]
#[tracing::instrument(skip_all, fields(digest = %digest, org = %query.org, ws = %query.ws, env = %query.env))]
#[debug_handler]
pub async fn delete_artifact(
    DeleteArtifactPath { digest }: DeleteArtifactPath,
    Query(query): Query<DeleteArtifactQuery>,
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
        crate::security::Scopes::of(&[Scope::Delete]),
        |c: &crate::security::Caller| c.can(Scope::Delete),
    );
    let placement =
        match Placement::resolve(&caller, requested, query.scope.as_deref()) {
            Ok(p) => p,
            Err(r) => return *r,
        };

    require_valid_digest!(&digest);

    validate_ownership!(state.provider, &tenant_code, &query);

    let hook_ctx = before_hook!(
        state,
        ArtifactOp::Delete,
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
    let x = state
        .storage_backend
        .delete(&container, &digest)
        .await
        .map_err(InternalServerError);

    let response = match x {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => e.into_response(),
    };

    after_response(&state, hook_ctx, OutcomeKind::Deleted, response).await
}
