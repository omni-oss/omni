# RFC 0030: authentication rollout

Implementation notes for the contributor continuing slice 2 of
[RFC 0030](0030-remote-cache-trust-model.md). The RFC remains the design authority.

## Assessment and scope

Large work, High risk: authentication crosses core routes, static and SQL/D1
providers, the authentication cache, hooks and the built-in client. The interfaces
are specified, but resource-specific SQL grants and cache invalidation need careful
integration. Code can be reverted; once the header changes, server and client must
be rolled back together. One implementation owner, independent Opus reviews before
each commit; no implementation delegation planned. The work spans several batches,
not a single edit. A new credential protocol or database migration would require
reassessment; OIDC itself is out of scope.

## Batches and verification

1. **Identity contract and chain (current).** Add types alongside the existing
   security service. First recognized identity wins even when denied; backend
   errors stop the chain; returned grants cannot exceed requested grants.
   Test fallthrough, denial, errors, resource and wanted-set forwarding, empty
   grants, an identity from a non-API-key authenticator, and UTF-8 identity limits.
   No active request path changes. Undo: remove the additive module.
2. **Providers and scoped caching.** Migrate static config and SQL/D1 identity
   resolution, including optional principal, keyed IDs and credential-cache keys
   covering resource plus wanted grants. Prove `all` does not imply publish,
   explicit publish does, and a cached identity cannot cross resources or wanted
   sets. Test revocation through TTL. Undo before activation: remove new providers.
   Split into static provider, SQL/D1, and caching commits to keep each review
   bounded. The static provider sub-batch adds an opt-in keyed-ID setting (also
   used by existing hooks), explicit principal and publish config, and the
   resource-scoped authenticator. Native/worker config wiring is included.
3. **Request migration.** Switch routes, hooks and `validate_access` to callers;
   move client and server together from `X-API-KEY` to Bearer credentials and remove
   obsolete security code. Test unknown identity (401), insufficient grants (403),
   backend unavailable (503), ownership validation, header rejection and
   `validate_access` results. Verify setup, native and worker integration in CI.
   Undo: revert server and client together. Namespace-dependent response fields
   must be coordinated with slice 4, not guessed.

Run focused tests during development. At integration run core, repository-query
and native-service tests, clippy with `-D warnings`, and format checks. Worker
target validation and the service TypeScript suites are left to CI. Do not run
`test:service:*` locally. Keep `.omni/sources/lock.json` out of commits.

## Decisions and status

- The contract first is preferred to replacing all routes in one change: it is
  independently testable and reversible, though unused until batch 3.
- `Scopes` has explicit grants only, empty by default. Providers, not the set,
  expand legacy `all` excluding publish. The chain narrows overbroad member grants.
- `CredentialKind` is a non-exhaustive enum with `ApiKey` and `Custom(String)`;
  future authenticators need no route-specific credential variants.
- Identity text (principal, credential kind/ID and optional pin) is non-empty,
  control-free and limited to 256 UTF-8 bytes. Namespace resolution still validates
  the pin's scope-label grammar. Trusted authenticators supply opaque identifiers,
  never raw credentials, and must not include secrets in backend errors.
- Plan and batch 1 implementation reviewed independently by Opus; no blocking
  findings. Review nits addressed: custom kind cannot impersonate `api_key`,
  backend error display is sanitized, and routes are documented to use the chain.
  Eight contract tests cover batch 1.
- **Not complete:** no routes or providers use the seam yet. Cache migration,
  SQL/D1 principal, Bearer headers and `validate_access` remain open.
- **Static provider sub-batch:** implemented and independently reviewed by Opus;
  no blockers. Review suggestions addressed: validate principals/empty secrets at
  config load, avoid indexing the key map, and test legacy scope denials as well
  as grants. Shared principals remain operator-controlled as designed by the RFC;
  a configured principal matching another key's default ID intentionally shares
  ownership, just like two matching configured principals. No client chooses it.
  `ConfigBasedDependencyProvider::authenticator` exposes a one-member chain,
  but routes still use the old service. Both services exclude publish from
  wildcard grants; explicit publish is independent of write. Config secret
  feeds both caller IDs and legacy hook IDs. Tests cover all four resource
  restrictions, requested grants, disabled/expired credentials, principal
  validation and rotation, config compatibility and native state wiring.
  Rollback: remove the static authenticator/config additions; deployments opting
  into a secret revert to unkeyed IDs, so correlate usage across the ID change.
- **SQL/D1 provider sub-batch:** implemented and independently reviewed by Opus;
  no blockers. Added the suggested invalid-key/malformed-principal short-circuit
  test and documented MySQL text decoding. The required executor method is a Rust
  API break; include it in release notes with the route/header migration.
  `QueryExecutor::api_key_identity` reads the existing `api_key_valid` query's
  `found` and optional `principal` in one execution. Missing/NULL principal keeps
  the old default; non-text or invalid identity data is an error. The new method is
  required so a third-party executor cannot silently ignore a principal column.
  Native SQL uses the existing bounded single-row reader and parameter binding;
  D1 uses its existing single-row reader. No schema or new query setting is added.
  `QueryDependencyProvider::authenticator` exposes the new one-member chain.
  Validity runs once per request, then ordered resource checks stop on denial,
  followed by one query per wanted scope. Scope query failures discard all grants.
  Operator scope queries must exclude publish from wildcard matching; the query
  provider cannot distinguish an explicit grant from an operator's wildcard SQL.
  Scripted tests check order, hashed parameters, short circuiting and failures at
  every query boundary; real SQLite tests check optional-column compatibility and
  malformed rows. Postgres/MySQL identity reads and D1 runtime remain CI gates.
  Routes and the authentication cache still use the old security service.
  Undo: remove the additive authenticator and executor method together; old
  `api_key_valid` queries remain valid, with or without a principal column.
- **Authentication-cache sub-batch:** implemented and independently reviewed by
  Opus; no blockers. Addressed import-layout nits and clarified the positive-TTL
  delay for new grants after a recognized denial. Worker CI remains the gate for
  `getrandom`'s wasm backend, which is configured by the workspace.
  Providers now expose a required `authenticator()` method; static/query providers
  return their chains, decorators forward/wrap it. `CachingState` retains a separate
  bounded `Option<Caller>` cache, shared by request-scoped providers belonging to
  the same authority. Keys are full HMAC-SHA256 over length-prefixed credential,
  tenant/org/workspace/environment and canonical wanted scopes; no raw credential
  is retained in the new cache's keys. Its 32-byte random pepper is generated per
  state using `getrandom` 0.3.4 (already used by the workspace; `wasm_js` on Workers),
  not the stable credential-ID secret. Randomness failure logs a warning and disables
  the caller cache, never falls back to unkeyed hashing.
  Errors are never stored. Recognized callers, including zero-grant denials, use
  the security TTL; unrecognized credentials use the optional negative TTL. Thus
  cached principals and removed grants remain stale until expiry. A cache hit does
  not slide expiry forward. Never share a state between independent authorities.
  Fake-clock tests check exact expiry and revocation, key separation, denial,
  errors, capacity and negative results; a scripted query provider proves rebuilt
  decorators share the state. The old bool-based cache was removed with the request migration (below).
  Undo: remove the additive caller cache and provider method together; legacy
  request authentication is unchanged. Worker randomness/runtime remains a CI gate.
- **Request-migration sub-batch:** implemented and independently reviewed by Opus,
  which found two blockers, both fixed. The `Credential` extractor reads exactly one
  `Authorization: Bearer` header; `X-API-KEY` is **not** accepted as a fallback (a clean
  break), and the built-in client sends Bearer. The token may be any visible ASCII
  (`0x21`-`0x7E`): API keys are operator-chosen strings, and the stricter RFC 6750 grammar
  would have locked out existing keys that contain symbols. Routes now run `authenticate!`
  first, then digest validation, then ownership, as the old `guard!` did: validating the
  target first would let an unauthenticated caller probe which organizations, workspaces
  and environments exist, and would cost a database query per anonymous request. The
  authenticator compares by value, so an unknown target yields `401`/`403`, never a grant.
  `authenticate!` asks the provider's (cached) authenticator for the one scope the route
  needs: unrecognized `401` with `WWW-Authenticate: Bearer`, recognized without the scope
  `403`, backend error `503` with a fixed body (the fact is logged, never the error text).
  Route spans skip all arguments and record only digest, org, ws and env. Hooks get the
  authenticated credential ID in the existing `api_key_id` field until slice 3.
  Tests use a non-API-key authenticator through all six routes, check that
  `security_service` and `api_key_id` are never called (those stubs were removed with the legacy code, so the compiler now enforces it), that hooks do not run before
  authentication, and that a missing `X-OMNI-TENANT` is still `400` (the extractor order
  is unchanged). Existing tests changed only in their headers and the `401` for unknown keys.
  Undo: revert
  server and client together (a client on the old header cannot authenticate).
- **Legacy-removal sub-batch:** implemented and independently reviewed by Opus (no
  code blockers; the stale guide sentences it flagged are fixed). Removed
  `SecurityService`, its error types, `InMemorySecurityService`, `QuerySecurityService`,
  `CachedSecurity` (and the separate boolean cache in `CachingState`), and the
  `DependencyProvider::security_service` and `api_key_id` methods. The static authenticator
  now applies the enabled/expiry and four resource checks itself. `caching.security`
  settings now configure only the caller cache. Hooks keep the existing `api_key_id`
  field (renamed in slice 3). Provider tests that compared against the old service now
  assert the authenticated behaviour directly. This is a Rust API break for third-party
  `DependencyProvider` implementers (they must drop the two methods). Callers of `provider.api_key_id()` use
  `Caller::credential().id()`, or `hooks::api_key_id_keyed` to compute an ID themselves.
  Mention this API break in the slice 2 release note.
- **Access-probe sub-batch:** implemented and independently reviewed by Opus (no
  blockers). Review follow-ups applied: `omni cache remote setup` refuses a credential
  whose reported scopes hold neither read nor write, with its own user diagnostic
  `remote_cache.insufficient_scopes` (the generic `invalid_access` hint would have misled,
  and deliberately hides the server's message); an older server that reports no scopes is
  accepted as before, an exhaustive-match test guards `Scopes::ALL` against new scope
  variants, and the size field is documented as advisory and unauthenticated. The `HEAD`
  list route (the probe behind `omni cache remote setup`) now asks the authenticator
  for every scope (`Scopes::ALL`, which includes publish) and is valid for any
  recognized caller with at least one scope for the target, instead of requiring
  `list:artifacts`. It answers `204` with `X-Omni-Scopes` (effective scopes, wire names,
  fixed order) and `X-Omni-Max-Artifact-Size` (`ServiceState::max_artifact_size`); a
  caller with no scope is `403`, unknown `401`, backend failure `503`, none of them with
  the headers. `ValidateAccessResult` gains `scopes` and `max_artifact_size`, filled from
  the headers only on success and never invented when absent or malformed; they are
  advisory. Authentication is now one function (`authenticate_caller`) used by the
  macro and the probe. This also adds the `503` log line the earlier review asked for
  (the first attempt missed after formatting). **Deferred to slice 4:** the principal ID
  and effective scope label in the response, because their definitions (namespace
  encoding, scope label grammar) land with namespaces; adding them now would fix a
  contract before it exists. Slice 2 is otherwise complete.
