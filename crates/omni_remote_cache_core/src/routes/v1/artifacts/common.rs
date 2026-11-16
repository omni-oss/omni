use axum::{
    Json,
    response::{IntoResponse, Response},
};
use http::StatusCode;
use serde::Deserialize;
use serde_json::json;
use utoipa::{IntoParams, ToSchema};

use omni_remote_cache_storage_core::Container;

use crate::{
    entry::Trailer,
    hooks::NamespaceKind,
    namespace::{Namespace, NamespaceError, ScopeLabel},
    security::Caller,
    services::Violation,
};

#[allow(unused)]
#[derive(Deserialize, IntoParams, Debug, ToSchema)]
#[into_params(parameter_in = Query)]
pub struct CommonArtifactQuery {
    pub org: String,
    pub ws: String,
    pub env: String,
}

/// Longest accepted `{digest}`. Real digests are base58 of a 32 byte hash
/// (at most 44 characters); the margin keeps older, longer ones working.
pub const MAX_DIGEST_LEN: usize = 64;

/// A digest becomes a storage key, so it is parsed at the boundary: base58
/// alphabet only (no `0`, `O`, `I`, `l`) and bounded. That excludes every path
/// separator, dot and percent escape, so it cannot name another location.
pub fn is_valid_digest(digest: &str) -> bool {
    !digest.is_empty()
        && digest.len() <= MAX_DIGEST_LEN
        && digest.bytes().all(|b| {
            matches!(
                b,
                b'1'..=b'9' | b'A'..=b'H' | b'J'..=b'N' | b'P'..=b'Z'
                    | b'a'..=b'k' | b'm'..=b'z'
            )
        })
}

/// Rejects a malformed `{digest}` with `400` before any path is built.
macro_rules! require_valid_digest {
    ($digest:expr) => {
        if !$crate::routes::v1::artifacts::common::is_valid_digest($digest) {
            return (
                ::http::StatusCode::BAD_REQUEST,
                "invalid digest: expected base58 of at most 64 characters",
            )
                .into_response();
        }
    };
}
pub(crate) use require_valid_digest;

#[inline(always)]
pub fn get_validation_response(
    violations: &[Violation],
    tenant: &str,
    org: &str,
    ws: &str,
    env: &str,
) -> Option<Response> {
    if violations.is_empty() {
        return None;
    }

    let violations = violations
        .iter()
        .copied()
        .map(|v| translate_violation(v, tenant, org, ws, env))
        .collect::<Vec<_>>();

    Some(
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "type": "https://httpstatuses.com/400",
                "title": "Validation Errors",
                "detail": violations.join(".\n"),
                "instance": "",
                "status": StatusCode::BAD_REQUEST.as_u16(),
                "violations": violations
            })),
        )
            .into_response(),
    )
}

fn translate_violation(
    violation: Violation,
    tenant: &str,
    org: &str,
    ws: &str,
    env: &str,
) -> String {
    match violation {
        Violation::TenantDoesNotExist => {
            format!("tenant for code '{}' does not exist", tenant)
        }
        Violation::TenantDoesNotHaveOrganization => {
            format!(
                "tenant for code '{}' does not have an organization with code '{}'",
                org, org
            )
        }
        Violation::OrganizationDoesNotExist => {
            format!("organization for code '{}' does not exist", org)
        }
        Violation::OrganizationDoesNotHaveWorkspace => {
            format!(
                "organization for code '{}' does not have a workspace with code '{}'",
                org, ws
            )
        }
        Violation::WorkspaceDoesNotExist => {
            format!("workspace for code '{}' does not exist", ws)
        }
        Violation::WorkspaceDoesNotHaveEnvironment => {
            format!(
                "workspace for code '{}' does not have an environment with code '{}'",
                ws, env
            )
        }
        Violation::EnvironmentDoesNotExist => {
            format!("environment for code '{}' does not exist", env)
        }
    }
}

pub fn forbidden_response(message: Option<&str>) -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(json!({
            "type": "https://httpstatuses.com/403",
            "title": "Forbidden",
            "detail": message.unwrap_or("Forbidden"),
            "instance": "",
            "status": StatusCode::FORBIDDEN.as_u16(),
        })),
    )
        .into_response()
}

/// Authenticates for `wanted` and returns the caller, or the (boxed) response to send:
/// `401` for an unrecognized credential and `503` when the authenticator fails.
/// The caller may still lack some or all of `wanted`; routes check that.
pub async fn authenticate_caller(
    provider: &dyn crate::providers::DependencyProvider,
    credential: &str,
    tenant: &str,
    query: &CommonArtifactQuery,
    wanted: crate::security::Scopes,
) -> Result<crate::security::Caller, Box<Response>> {
    let resource = crate::security::Resource {
        tenant: tenant.to_string(),
        org: query.org.clone(),
        ws: query.ws.clone(),
        env: query.env.clone(),
    };
    match provider
        .authenticator()
        .authenticate(credential, &resource, wanted)
        .await
    {
        Ok(Some(caller)) => Ok(caller),
        Ok(None) => Err(Box::new(crate::extractors::unauthorized_response())),
        Err(_) => {
            // The error text may contain credentials, so only the fact is logged.
            tracing::error!("authenticator backend failed");
            Err(Box::new(
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "authentication unavailable",
                )
                    .into_response(),
            ))
        }
    }
}

pub fn not_authorized_response() -> Response {
    forbidden_response(Some("You are not authorized to process this request"))
}

/// Authenticates for `wanted` and requires `allowed(&caller)`: evaluates to the
/// caller, or returns `401` (unrecognized), `403` (recognized without what the
/// request needs) or `503` (the authenticator failed). What a request needs
/// depends on the namespace it names, so each route says it explicitly.
pub macro authorize(
        $provider:expr,
        $credential:expr,
        $tenant_code:expr,
        $query:expr,
        $wanted:expr,
        $allowed:expr$(,)?
    ) {{
    use crate::routes::v1::artifacts::common::{
        authenticate_caller, not_authorized_response,
    };
    match authenticate_caller(
        $provider.as_ref(),
        $credential,
        $tenant_code,
        &CommonArtifactQuery {
            org: $query.org.clone(),
            ws: $query.ws.clone(),
            env: $query.env.clone(),
        },
        $wanted,
    )
    .await
    {
        Ok(caller) if $allowed(&caller) => caller,
        Ok(_) => return not_authorized_response(),
        Err(response) => return *response,
    }
}}

/// Which namespace a request names. `PUT` calls it `target`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum NamespaceParam {
    Shared,
    Isolated,
}

/// Where a request reads or writes, resolved from the authenticated caller and
/// the validated request. The container is built only from this.
#[derive(Debug, Clone)]
pub struct Placement {
    namespace: Namespace,
    label: Option<ScopeLabel>,
}

fn bad_request(message: &'static str) -> Box<Response> {
    Box::new((StatusCode::BAD_REQUEST, message).into_response())
}

fn server_error(message: &'static str) -> Box<Response> {
    Box::new((StatusCode::INTERNAL_SERVER_ERROR, message).into_response())
}

impl Placement {
    /// `scope_param` is the request's `scope`. It is ignored for the shared
    /// namespace, and when the caller carries a `scope_pin`, which always wins.
    pub fn resolve(
        caller: &Caller,
        requested: NamespaceParam,
        scope_param: Option<&str>,
    ) -> Result<Self, Box<Response>> {
        if requested == NamespaceParam::Shared {
            return Ok(Self {
                namespace: Namespace::Shared,
                label: None,
            });
        }
        let label = Self::label(caller, scope_param)?;
        let namespace = Namespace::isolated(caller.principal(), &label)
            .map_err(|_| {
                // The authenticator vouched for an identity that cannot name a
                // namespace: not the client's fault.
                tracing::error!(
                    "authenticated principal cannot form a namespace"
                );
                server_error("invalid authenticated identity")
            })?;
        Ok(Self {
            namespace,
            label: Some(label),
        })
    }

    /// The scope label for `caller`: its pin, else the request's value, else
    /// `default`.
    pub fn label(
        caller: &Caller,
        scope_param: Option<&str>,
    ) -> Result<ScopeLabel, Box<Response>> {
        if let Some(pin) = caller.scope_pin() {
            // A pin that is not a valid label is an authenticator fault. It is
            // never replaced by the client's value and never hashed to fit.
            return ScopeLabel::parse(pin).map_err(|_| {
                tracing::error!("authenticator returned an invalid scope pin");
                server_error("invalid authenticated identity")
            });
        }
        match scope_param {
            Some(label) => ScopeLabel::parse(label).map_err(|_| {
                bad_request(
                    "scope must be 1 to 64 letters, digits, '.', '_' or '-', starting with a letter or digit",
                )
            }),
            None => Ok(ScopeLabel::default_label()),
        }
    }

    pub fn namespace(&self) -> &Namespace {
        &self.namespace
    }

    pub fn kind(&self) -> NamespaceKind {
        self.namespace.kind()
    }

    /// The label for hooks and the trailer: only an isolated entry has one.
    pub fn scope(&self) -> Option<String> {
        self.label.as_ref().map(|l| l.as_str().to_owned())
    }

    pub fn container(
        &self,
        tenant: &str,
        query: &CommonArtifactQuery,
    ) -> Result<Container, Box<Response>> {
        self.namespace
            .container(tenant, &query.org, &query.ws, &query.env)
            .map_err(namespace_error)
    }

    /// Whether a stored trailer belongs where this request looked. A shared hit
    /// must say `shared`; an isolated one must name this principal and scope.
    /// Anything else is a miss, which defeats bytes copied between prefixes.
    pub fn owns(&self, trailer: &Trailer, principal: &str) -> bool {
        match self.kind() {
            NamespaceKind::Shared => trailer.namespace == "shared",
            NamespaceKind::Isolated => {
                trailer.namespace == "isolated"
                    && trailer.writer_principal == principal
                    && trailer.scope.as_deref()
                        == self.label.as_ref().map(ScopeLabel::as_str)
            }
        }
    }

    /// The `namespace` the trailer records.
    pub fn trailer_namespace(&self) -> &'static str {
        match self.kind() {
            NamespaceKind::Shared => "shared",
            NamespaceKind::Isolated => "isolated",
        }
    }
}

/// Lists the namespace a request names. A shared list is the environment's
/// entries; an isolated one covers every scope of the caller's own principal.
pub async fn list_namespace(
    storage: &crate::state::DynStorageBackend,
    placement: &Placement,
    tenant: &str,
    query: &CommonArtifactQuery,
) -> Result<Vec<super::get_artifacts::CacheItem>, Box<Response>> {
    use super::get_artifacts::CacheItem;
    use omni_remote_cache_storage_core::LIST_PAGE_SIZE;

    let prefix = placement
        .namespace()
        .list_prefix(tenant, &query.org, &query.ws, &query.env)
        .map_err(namespace_error)?;
    let isolated = placement.kind() == NamespaceKind::Isolated;
    let mut items = Vec::new();
    let mut after: Option<String> = None;
    loop {
        let page = storage
            .list_prefix(&prefix, after.as_deref(), LIST_PAGE_SIZE)
            .await
            .map_err(|e| {
                Box::new(
                    axum_extra::response::InternalServerError(e)
                        .into_response(),
                )
            })?;
        for item in page.items {
            let size = item.size().as_u64();
            let key = item.key();
            let (scope, digest) = if isolated {
                match key.split_once('/') {
                    Some((scope, digest)) => (Some(scope.to_owned()), digest),
                    None => continue,
                }
            } else if key.contains('/') {
                continue;
            } else {
                (None, key)
            };
            items.push(CacheItem {
                digest: digest.to_owned(),
                size,
                scope,
            });
        }
        match page.next_after {
            Some(next) => after = Some(next),
            None => return Ok(items),
        }
    }
}

/// A resource code that is too long is the client's request; anything else
/// means the server built something it should not have.
pub fn namespace_error(error: NamespaceError) -> Box<Response> {
    match error {
        NamespaceError::CodeTooLong => {
            bad_request("a resource code is too long")
        }
        NamespaceError::KeyTooLong => {
            bad_request("the resource codes are too long together")
        }
        NamespaceError::InvalidScope => bad_request("invalid scope"),
        NamespaceError::InvalidPrincipal | NamespaceError::InvalidContainer => {
            tracing::error!(%error, "could not build a storage container");
            server_error("could not build the storage location")
        }
    }
}

pub macro validate_ownership(
        $provider:expr,
        $tenant_code:expr,
        $query:expr$(,)?
    ) {{
    use crate::routes::v1::artifacts::common::get_validation_response;
    use axum::response::IntoResponse as _;
    use axum_extra::response::InternalServerError;

    let validate_svc = $provider.validation_service();

    let result = validate_svc
        .validate_ownership($tenant_code, &$query.org, &$query.ws, &$query.env)
        .await
        .map_err(InternalServerError);

    match result {
        Ok(r) => {
            if let Some(response) = get_validation_response(
                r.violations(),
                &$tenant_code,
                &$query.org,
                &$query.ws,
                &$query.env,
            ) {
                return response;
            }
        }
        Err(e) => return e.into_response(),
    }
}}

/// Sets the provenance headers `GET` returns for a stored entry. Only what a
/// reader needs: where the entry came from and what it hashes to. Who wrote it
/// is for operators (`list:artifacts` or `delete:artifacts`) and is not sent
/// here.
pub fn add_provenance_headers(
    headers: &mut http::HeaderMap,
    trailer: &crate::entry::Trailer,
) {
    use http::{HeaderName, HeaderValue};
    let mut set = |name: &'static str, value: String| {
        // Server-authored text that is not a valid header value is left out
        // rather than failing a read.
        if let Ok(value) = HeaderValue::from_str(&value) {
            headers.insert(HeaderName::from_static(name), value);
        }
    };
    set("x-omni-namespace", trailer.namespace.clone());
    if let Ok(stored_at) = trailer
        .stored_at
        .format(&time::format_description::well_known::Rfc3339)
    {
        set("x-omni-stored-at", stored_at);
    }
    set("x-omni-content-sha256", trailer.sha256.clone());
    if let Some(signature) =
        trailer.signature.as_ref().filter(|s| s.len() <= 256)
    {
        set("x-omni-signature", signature.clone());
    }
}

/// Maps a response status to the outcome the `after` hook reports.
pub fn outcome_kind(
    status: StatusCode,
    success: crate::hooks::OutcomeKind,
) -> crate::hooks::OutcomeKind {
    use crate::hooks::OutcomeKind;
    if status == StatusCode::NOT_FOUND {
        OutcomeKind::Miss
    } else if status == StatusCode::CONFLICT {
        OutcomeKind::AlreadyExists
    } else if status.is_success() {
        success
    } else {
        OutcomeKind::Failed
    }
}

/// Schedules `after` from the final response and passes the response through.
pub async fn after_response(
    state: &crate::state::ServiceState,
    ctx: crate::hooks::HookContext,
    success: crate::hooks::OutcomeKind,
    response: Response,
) -> Response {
    after_response_with(state, ctx, success, response, None).await
}

/// As [`after_response`], for an operation that has the artifact's SHA-256 to
/// report. It is attached only when the operation succeeded in the way the
/// caller expected (`Stored`, `Hit`); a conflict, miss or failure reports none, so
/// a hook never records the hash of an entry that was not written or served.
pub async fn after_response_with(
    state: &crate::state::ServiceState,
    ctx: crate::hooks::HookContext,
    success: crate::hooks::OutcomeKind,
    response: Response,
    sha256: Option<String>,
) -> Response {
    use crate::hooks::{HookOutcome, OutcomeKind, run_after};

    let kind = outcome_kind(response.status(), success);
    let sha256 = sha256
        .filter(|_| matches!(kind, OutcomeKind::Stored | OutcomeKind::Hit));
    run_after(state.provider.as_ref(), ctx, HookOutcome { kind, sha256 }).await;

    response
}

/// Builds the hook context and runs `before`. Returns early with the veto or
/// failure response, otherwise evaluates to the context for `after`.
pub macro before_hook(
        $state:expr,
        $op:expr,
        $caller:expr,
        $tenant_code:expr,
        $query:expr,
        $digests:expr,
        $placement:expr$(,)?
    ) {{
    use crate::hooks::{HookContext, run_before};

    let ctx = HookContext {
        op: $op,
        tenant: $tenant_code.to_string(),
        org: $query.org.clone(),
        ws: $query.ws.clone(),
        env: $query.env.clone(),
        digests: $digests,
        caller_id: $caller.credential().id().to_owned(),
        credential_kind: $caller.credential().kind().clone(),
        principal: $caller.principal().to_owned(),
        namespace: $placement.kind(),
        scope: $placement.scope(),
    };

    if let Some(response) = run_before($state.provider.as_ref(), &ctx).await {
        return response;
    }

    ctx
}}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shared container of one environment, as the routes build it.
    fn container(
        tenant: &str,
        org: &str,
        ws: &str,
        env: &str,
    ) -> Result<Container, NamespaceError> {
        Namespace::Shared.container(tenant, org, ws, env)
    }

    #[test]
    fn outcome_kind_reports_conflicts_as_already_exists() {
        use super::outcome_kind;
        use crate::hooks::OutcomeKind::*;
        use http::StatusCode as S;
        assert_eq!(outcome_kind(S::NO_CONTENT, Stored), Stored);
        assert_eq!(outcome_kind(S::CONFLICT, Stored), AlreadyExists);
        assert_eq!(outcome_kind(S::NOT_FOUND, Hit), Miss);
        assert_eq!(outcome_kind(S::BAD_REQUEST, Stored), Failed);
        assert_eq!(outcome_kind(S::INTERNAL_SERVER_ERROR, Stored), Failed);
    }

    #[test]
    fn digests_must_be_bounded_base58() {
        use super::is_valid_digest as ok;
        assert!(ok("abc"));
        assert!(ok(
            "1234567891234567891234567891234567891234567891234567891234567891"
        ));
        for bad in [
            "",
            "0",
            "O",
            "I",
            "l",
            "ab-c",
            "ab.c",
            "a/b",
            "..",
            "a%2Fb",
            "a b",
            "..%2F..%2Fshared%2FX",
            "é",
        ] {
            assert!(!ok(bad), "{bad:?} must be rejected");
        }
        assert!(!ok(&"a".repeat(65)));
        assert!(ok(&"a".repeat(64)));
    }

    #[test]
    fn dot_and_empty_codes_are_escaped_into_a_valid_container() {
        for bad in ["", ".", ".."] {
            let c = container("t", bad, "w", "e").unwrap();
            assert!(
                !c.as_str()
                    .split('/')
                    .any(|s| s.is_empty() || s == "." || s == "..")
            );
        }
    }

    #[test]
    fn tenants_never_share_a_container() {
        assert_ne!(
            container("t1", "o", "w", "e").unwrap(),
            container("t2", "o", "w", "e").unwrap()
        );
        assert_ne!(
            container("a/b", "c", "w", "e").unwrap(),
            container("a", "b/c", "w", "e").unwrap()
        );
    }

    #[test]
    fn container_does_not_collide_on_percent_encoded_names() {
        assert_ne!(
            container("t", "foo/bar", "ws", "env").unwrap(),
            container("t", "foo%2Fbar", "ws", "env").unwrap()
        );
        assert_ne!(
            container("t", "a", "b/c", "d").unwrap(),
            container("t", "a/b", "c", "d").unwrap()
        );
        assert_ne!(
            container("t", "CON", "ws", "env").unwrap(),
            container("t", "_CON", "ws", "env").unwrap()
        );
    }
}
