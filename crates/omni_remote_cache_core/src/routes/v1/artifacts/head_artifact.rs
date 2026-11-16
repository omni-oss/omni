use axum::{
    extract::{Query, State},
    response::{IntoResponse, Response},
};
use axum_extra::{response::InternalServerError, routing::TypedPath};
use http::StatusCode;
use serde::Deserialize;
use utoipa::IntoParams;

use super::common::{container, require_valid_digest};
use crate::{
    entry::{EntryRead, read_entry},
    extractors::{Credential, TenantCode},
    hooks::{ArtifactOp, OutcomeKind},
    routes::v1::artifacts::common::{
        after_response, authenticate, before_hook, validate_ownership,
    },
    security::Scope,
    state::ServiceState,
};

#[derive(TypedPath, Deserialize, Debug)]
#[typed_path("/{digest}")]
pub struct HeadArtifactPath {
    pub digest: String,
}

#[derive(Deserialize, IntoParams, Debug)]
#[into_params(parameter_in = Query)]
pub struct HeadArtifactQuery {
    /// The organization code
    pub org: String,
    /// The workspace code
    pub ws: String,
    /// The environment code
    pub env: String,
}

#[utoipa::path(
    head,
    path = "/{digest}",
    description = "Download an artifact",
    params(
        ("digest" = String, Path, description = "Artifact digest"),
        ("X-OMNI-TENANT" = String, Header, description = "Tenant code"),
        HeadArtifactQuery
    ),
    responses(
        (
            status = NO_CONTENT,
            description = "Success",
        ),
        (status = NOT_FOUND, description = "Not found"),
        (status = BAD_REQUEST, description = "Bad request"),
        (status = INTERNAL_SERVER_ERROR, description = "Internal server error")
    )
)]
#[tracing::instrument(skip_all, fields(digest = %digest, org = %query.org, ws = %query.ws, env = %query.env))]
pub async fn head_artifact(
    HeadArtifactPath { digest }: HeadArtifactPath,
    Query(query): Query<HeadArtifactQuery>,
    TenantCode(tenant_code): TenantCode,
    State(state): State<ServiceState>,
    Credential(credential): Credential,
) -> Response {
    let caller = authenticate!(
        state.provider,
        &credential,
        &tenant_code,
        &query,
        Scope::Read,
    );

    require_valid_digest!(&digest);

    validate_ownership!(state.provider, &tenant_code, &query);

    let hook_ctx = before_hook!(
        state,
        ArtifactOp::Head,
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
    // The same check as `GET`, so the two agree about which entries exist.
    let response = match read_entry(
        state.storage_backend.as_ref(),
        &container,
        &digest,
    )
    .await
    {
        Ok(EntryRead::Valid(_)) => StatusCode::NO_CONTENT.into_response(),
        Ok(EntryRead::Absent) => StatusCode::NOT_FOUND.into_response(),
        Ok(EntryRead::Invalid(why)) => {
            tracing::warn!(%why, "ignoring a stored object that is not a valid entry");
            StatusCode::NOT_FOUND.into_response()
        }
        Err(e) => InternalServerError(e).into_response(),
    };

    after_response(&state, hook_ctx, OutcomeKind::Hit, response).await
}
