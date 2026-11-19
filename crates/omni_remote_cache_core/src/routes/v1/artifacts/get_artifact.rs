use axum::{
    body::Body,
    extract::{Query, State},
    response::{IntoResponse, Response},
};
use axum_extra::{response::InternalServerError, routing::TypedPath};
use http::{StatusCode, header};
use serde::Deserialize;
use utoipa::IntoParams;

use super::common::{
    CommonArtifactQuery, NamespaceParam, Placement, add_provenance_headers,
    after_response_with, require_valid_digest,
};
use crate::{
    entry::{EntryRead, checked_body, read_entry},
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
pub struct GetArtifactPath {
    pub digest: String,
}

#[derive(Deserialize, IntoParams, Debug)]
#[into_params(parameter_in = Query)]
pub struct GetArtifactQuery {
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
    get,
    path = "/{digest}",
    description = "Download an artifact",
    params(
        ("digest" = String, Path, description = "Artifact digest"),
        ("X-OMNI-TENANT" = String, Header, description = "Tenant code"),
        GetArtifactQuery
    ),
    responses(
        (
            status = OK,
            description = "Success",
            content_type = "application/octet-stream",
        ),
        (status = NOT_FOUND, description = "Not found"),
        (status = BAD_REQUEST, description = "Bad request"),
        (status = INTERNAL_SERVER_ERROR, description = "Internal server error")
    )
)]
#[tracing::instrument(skip_all, fields(digest = %digest, org = %query.org, ws = %query.ws, env = %query.env))]
pub async fn get_artifact(
    GetArtifactPath { digest }: GetArtifactPath,
    Query(query): Query<GetArtifactQuery>,
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
        ArtifactOp::Get,
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
    // Only a well-formed sealed entry is a hit: an object with no valid trailer
    // (for example one stored before trailers existed) is a miss.
    let entry = match read_entry(
        state.storage_backend.as_ref(),
        &container,
        &digest,
    )
    .await
    {
        Ok(EntryRead::Valid(entry))
            if placement.owns(&entry.parsed.trailer, caller.principal()) =>
        {
            Some(entry)
        }
        Ok(EntryRead::Valid(_)) => None,
        Ok(EntryRead::Absent) => None,
        Ok(EntryRead::Invalid(why)) => {
            tracing::warn!(%why, "ignoring a stored object that is not a valid entry");
            None
        }
        Err(e) => {
            let response = InternalServerError(e).into_response();
            return after_response(
                &state,
                hook_ctx,
                OutcomeKind::Hit,
                response,
            )
            .await;
        }
    };

    // The hash a hit reports to the hooks is the stored entry's own.
    let sha256 = entry
        .as_ref()
        .map(|entry| entry.parsed.trailer.sha256.clone());
    let response = match entry {
        None => (StatusCode::NOT_FOUND, "Not found").into_response(),
        Some(entry) => {
            match state
                .storage_backend
                .get_stream(&container, &digest)
                .await
                .map_err(InternalServerError)
            {
                // Removed between the two reads.
                Ok(None) => {
                    (StatusCode::NOT_FOUND, "Not found").into_response()
                }
                Ok(Some(stream)) => {
                    let trailer = &entry.parsed.trailer;
                    let mut response = Response::builder()
                        .status(StatusCode::OK)
                        .header(
                            header::CONTENT_TYPE,
                            "application/octet-stream",
                        )
                        .header(
                            header::CONTENT_DISPOSITION,
                            format!("attachment; filename=\"{}\"", digest),
                        )
                        // The body is exactly the artifact. If the stored
                        // object turns out not to match what was read, the body
                        // errors and this length makes that a failed transfer.
                        .header(header::CONTENT_LENGTH, trailer.size)
                        .body(Body::from_stream(checked_body(stream, &entry)))
                        .expect("should be able to build response from stream");
                    add_provenance_headers(response.headers_mut(), trailer);
                    response
                }
                Err(e) => e.into_response(),
            }
        }
    };

    after_response_with(&state, hook_ctx, OutcomeKind::Hit, response, sha256)
        .await
}
