use axum::{
    extract::{Query, State},
    response::{IntoResponse, Response},
};
use axum_extra::{response::InternalServerError, routing::TypedPath};
use http::StatusCode;
use serde::Deserialize;
use utoipa::IntoParams;

use super::common::{
    CommonArtifactQuery, NamespaceParam, Placement, require_valid_digest,
};
use crate::{
    entry::{EntryRead, read_entry},
    extractors::{Credential, TenantCode},
    hooks::{ArtifactOp, OutcomeKind},
    routes::v1::artifacts::common::{
        after_response, authorize, before_hook, validate_ownership,
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
    /// `shared` (default) or `isolated`
    pub namespace: Option<NamespaceParam>,
    /// Scope label for the isolated namespace
    pub scope: Option<String>,
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
    let requested = query.namespace.unwrap_or(NamespaceParam::Shared);
    let caller = match requested {
        NamespaceParam::Shared => authorize!(
            state.provider,
            &credential,
            &tenant_code,
            &query,
            crate::security::Scopes::of(&[Scope::Read]),
            |c: &crate::security::Caller| c.can(Scope::Read),
        ),
        NamespaceParam::Isolated => authorize!(
            state.provider,
            &credential,
            &tenant_code,
            &query,
            crate::security::Scopes::of(&[
                Scope::Read,
                Scope::Write,
                Scope::Publish
            ]),
            |c: &crate::security::Caller| c.can_read_isolated(),
        ),
    };
    let placement =
        match Placement::resolve(&caller, requested, query.scope.as_deref()) {
            Ok(p) => p,
            Err(r) => return *r,
        };

    require_valid_digest!(&digest);

    validate_ownership!(state.provider, &tenant_code, &query);

    let hook_ctx = before_hook!(
        state,
        ArtifactOp::Head,
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
    // The same check as `GET`, so the two agree about which entries exist.
    let response = match read_entry(
        state.storage_backend.as_ref(),
        &container,
        &digest,
    )
    .await
    {
        Ok(EntryRead::Valid(entry))
            if placement.owns(&entry.parsed.trailer, caller.principal()) =>
        {
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(EntryRead::Valid(_)) => StatusCode::NOT_FOUND.into_response(),
        Ok(EntryRead::Absent) => StatusCode::NOT_FOUND.into_response(),
        Ok(EntryRead::Invalid(why)) => {
            tracing::warn!(%why, "ignoring a stored object that is not a valid entry");
            StatusCode::NOT_FOUND.into_response()
        }
        Err(e) => InternalServerError(e).into_response(),
    };

    after_response(&state, hook_ctx, OutcomeKind::Hit, response).await
}
