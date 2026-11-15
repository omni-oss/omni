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
