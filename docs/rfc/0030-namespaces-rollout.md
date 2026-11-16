# RFC 0030: scopes and namespaces rollout

Implementation plan for slice 4 of [RFC 0030](0030-remote-cache-trust-model.md) (section 4.1,
section 4.1.1 and the namespace parts of 4.8.4). The RFC stays the design authority. This records
the decisions the RFC leaves open, the batches, and what each proves.

## Assessment

Large work, High risk: it changes where every entry is stored (persistent data, so old entries
become unreachable), what each scope allows on every route (authorization), and it is the part of
the design that separates trusted from untrusted writers. One implementation owner, an independent
Opus review of this plan, then of each batch before its commit.

Already in place from earlier batches: the typed `Container`/`ContainerPrefix`, path-component
escaping, base58 digests, `list_prefix`/`list_children`/`delete_prefix`, the `Caller` (principal,
optional `scope_pin`), the `publish:artifacts` scope (never implied by `all`), and trailers that
carry `namespace` and `scope`. The `multi_bucket` flag is already gone.

## Goal and done

- Entries live under `shared/{t}/{o}/{w}/{e}` or `isolated/{t}/{o}/{w}/{e}/{principal_id}/{scope}`.
  The namespace is chosen by the server from the authenticated caller and a validated request, never
  from a path the client built, so one principal can never write into another's namespace and an
  isolated writer can never reach `shared/`.
- Scopes mean what the RFC table says on every route, with tests that a read-only key can never
  write, that `write` cannot write shared, that `publish` is required for shared, and that an
  isolated entry is only ever read by its own principal and scope.
- The trailer and the hooks record the real `namespace` and `scope`.
- The access probe also reports the principal ID and the effective scope label (deferred from
  slice 2).

Out of scope: client placement policy and config (`push`, `target`, `scope: auto`, probe order,
slice 5); signatures (slice 7); purge and provenance routes, including deleting another principal's
isolated entries (slice 8).

## Decisions

1. **Namespace type.** A core `Namespace` (`Shared`, `Isolated { principal_id, scope }`) builds the
   `Container`; nothing else builds one for the artifact routes. The resource codes keep the
   existing injective escaping, so a code cannot add a segment, and the first segment is always
   `shared` or `isolated`. Any escaped code over 255 bytes, or a whole key that could exceed 1024
   bytes (S3's limit; the budget is checked when the container is built), is `400`.
2. **Path segments for `principal_id` and `scope`.** A segment is the value itself only when it
   is a _safe literal_: lowercase letters and digits first, then `[a-z0-9._-]`, at most 64
   characters, no trailing `.`, not a Windows device name (`CON`, `PRN`, `AUX`, `NUL`, `COM1-9`,
   `LPT1-9`, with or without an extension), and not starting with `h-`. Anything else becomes
   `h-` plus 32 lowercase hex characters of the SHA-256 of the original text. This deviates from the
   RFC's case-preserving rule on purpose (recorded in section 8): on a case-insensitive filesystem
   (macOS, Windows) `Ab` and `ab` are one directory, and on Windows `a.` is `a`, so a literal that
   differs only in case, or a hashed form that a literal could imitate, would let two principals
   read each other's isolated entries. Literals are lowercase and never start with `h-`; hashed
   segments are `h-` plus lowercase hex, so the two forms are disjoint and injective. The readable
   principal and label are stored in the trailer. A principal may be any text the `Caller` accepts
   (up to 256 bytes, no control characters); a `scope` label must match the RFC's
   `[A-Za-z0-9][A-Za-z0-9._-]{0,63}`, otherwise `400`.
3. **Scope label.** The label is the caller's pin when it has one, else the request's `scope`
   (validated after authentication, ignored for the shared namespace), else `default`. A pin that
   is not a valid label is an authenticator fault: the request fails closed with `500` and an
   error log without the value. It never falls back to the client's value and is never hashed to
   make it fit.
4. **Request parameters.** `PUT` takes `target=shared|isolated` (default `isolated`); `GET`,
   `HEAD`, `DELETE` and the list take `namespace=shared|isolated` (default `shared`); an isolated
   request may carry `scope`. An unknown `target` or `namespace` is `400` (the query extractor;
   it reveals nothing about resources). The principal is never read from a request.
5. **Authorization.** "Publish implies write" is one rule in one place (`Scopes`/`Caller`
   helpers), used for every route and for the probe. The wanted set per request:
   - `PUT target=isolated`: `write` or `publish`.
   - `PUT target=shared`: `publish`; otherwise `403`. `write` alone, and a legacy `all` key, can
     never write shared.
   - `GET`/`HEAD namespace=shared`: `read`.
   - `GET`/`HEAD namespace=isolated`: `read` and (`write` or `publish`): the caller's own entries.
   - list: `list`, namespace-aware (decision 8).
   - `DELETE`: `delete`, in `shared` or the caller's own isolated namespace. Deleting another
     principal's entries is the purge route (slice 8); this narrows the RFC's "any namespace, for
     operators" and is recorded in section 8.
6. **Isolated reads check the trailer.** A hit on an isolated entry requires the trailer's
   namespace, principal and scope to equal the request's; anything else is a miss, which defeats
   bytes copied between prefixes at the storage level. A shared hit requires namespace `shared`.
7. **Trailer and hooks.** The trailer records `namespace` (`shared` or `isolated`), the readable
   principal and, for isolated, the label as `scope`. The hook context carries `NamespaceKind` and
   `scope: Some(label)` for isolated and `None` for shared, even if the client sent one.
8. **List.** `GET /artifacts` and `HEAD /artifacts` are namespace-aware. An isolated list is the
   caller's whole principal (the `isolated_principal` prefix, every scope) and needs `list` only;
   `CacheItem` gains `scope` (the storage segment, so entries of different scopes are not
   ambiguous). A shared list is the shared prefix. This is part of batch 2: otherwise `GET /` would
   keep listing the old container and show entries that cannot be fetched.
9. **Probe.** The access probe accepts an optional `scope`, and adds `X-Omni-Principal-Id` (the
   path segment) and `X-Omni-Scope` (the effective label: the pin, else the validated client
   value, else `default`), which slice 5 needs to name its partitions.
10. **Legacy container.** The old `t/o/w/e` container is never read or written again. Its entries
    become unreachable and are not deleted. `omni cache purge --legacy` (slice 8) must tell them
    apart from a tenant literally named `shared` or `isolated`; recorded for that slice. The
    release note says it.
11. **Built-in client (interim).** Until slice 5 brings the configured policy, the client pushes to
    `target=isolated` with scope `default` and reads `isolated` first, then `shared` (an
    `isolated` miss, `403` or `5xx` falls back to `shared`). Under the default principal a
    single-key setup behaves as it does today, it needs no `publish`, and it matches the RFC's
    eventual order. An explicit placement argument on the client trait lets tests and slice 5 push
    to `shared`. Fixture keys that exercise the shared path gain `publish:artifacts`.
12. **Storage.** Namespace prefixes are what a bucket lifecycle rule matches (`isolated/`). The
    backends already take a `Container`; tests prove a shared and an isolated entry with the same
    digest never collide and that a prefix operation on one namespace cannot touch the other.
    Known limit: the existing code escaping is case-sensitive for `tenant/org/ws/env`, so on a
    case-insensitive filesystem two codes differing only in case alias; pre-existing, recorded.

## Batches

1. **Namespace type and container construction.** Pure and additive: segment derivation
   (decision 2), `Namespace`, the container builder and `ContainerPrefix` helpers, the length
   budgets. Tests: the safe-literal grammar at every boundary (64 and 65 characters, characters
   versus bytes, non-ASCII, empty, leading punctuation, `.` and `..`, trailing dot, uppercase,
   device names with and without an extension); the hash (stable, 32 hex, `Ab` and `ab` differ and
   neither aliases the other, a principal that is literally `h-` plus hex is hashed and cannot
   equal another's hashed form, two principals sharing a long prefix differ); codes containing
   `/`, `%`, `..` and trailing dots; over-long codes and keys are errors; and no input yields a
   container outside its namespace, or one equal to another namespace's. Undo: delete the module.
2. **Routes, client and fixtures.** Every route builds its container from the authenticated caller
   (decisions 3 to 8), applies the authorization of decision 5 through one helper, checks isolated
   trailers (decision 6), populates trailer and hooks (decision 7) and the probe (decision 9); the
   client follows decision 11. Tests through the real routes with the local backend and a
   non-API-key authenticator that sets a principal and optionally a pin: every scope against every
   route and namespace (the full matrix, `write` and `publish` separately); a read-only key can
   never write; `all` without `publish` gets `403` on a shared `PUT`; two principals, and two
   scopes of one principal, are invisible to each other; principals differing only in case, and a
   principal of the form `h-<hex>`, stay separate; a pin overrides a client `scope`; an invalid pin
   fails closed; a client `scope` containing `..` or `/` is `400`; a `scope` on a shared request is
   ignored; the same digest in both namespaces coexists and a `409` is per namespace; a copied
   object whose trailer names another namespace, principal or scope is a miss; the trailer, hook
   and probe facts; lists (shared, isolated across scopes, `scope` field); and the legacy container
   is not served. The TypeScript service suite, the `omni-tests` cache e2e, `hybrid.rs`, the client
   tests and all service tests move in this batch. Undo: revert server and client together.

Checks per batch: the unit and integration tests for the touched crates, clippy with `-D warnings`,
formatting and the wasm32 worker check. The `test:service:*` suites, the `omni-tests` e2e and the
client's live-service tests run in CI.

## Risks and how they are covered

- **Cross-principal access.** The principal comes only from the `Caller`; the container is built
  only by core from it. A test per route sends every client-controlled input that could name another
  principal or namespace and shows none changes the container.
- **Segment collisions.** `principal_id` hashing and the existing code escaping are injective and
  tested for the cases above; the namespace is always the first segment.
- **Scope confusion.** The matrix test covers every scope against every route and namespace, with
  `publish` and `write` separately, so a route that checks the wrong scope fails a named case.
- **Interim client.** A fixed `shared` policy needs `publish`; this is recorded, tested against the
  live fixtures, and replaced in slice 5.

## Status

Plan reviewed by Opus (six blockers, all folded in above: the `h-` collision and case aliasing
(decision 2), the pin grammar (decision 3), the length budgets (decision 1), the interim client
(decision 11) and the probe label (decision 9)). Its answers to the open questions are adopted:
publish implies write everywhere, the default label is `default`, and the interim client pushes
`isolated` and reads `isolated` then `shared`. RFC section 8 deviations to record: the lowercase
and `h-` segment rule, delete limited to shared and the caller's own isolated namespace for now,
and the case-sensitive code escaping.

- **Batch 1 (namespace type and container construction): implemented and independently reviewed by
  Opus (no blockers).** New `namespace` module in `omni_remote_cache_core`: `segment` and
  `is_safe_literal` (decision 2), `ScopeLabel` (a label validated once, so a pin or request value
  cannot reach a path unchecked), `Namespace` with `container` and `list_prefix`, and the length
  budgets (`MAX_CODE_LEN` after escaping, `MAX_KEY_LEN` with a 64 byte digest, checked at the exact
  boundary). Review follow-ups: constants and the identity-text check are shared with the code that
  already owns them, a Unicode normalization form is a separate principal (so an identity provider
  that is inconsistent gives one person two namespaces, which is safe), and the tests that could
  not fail were replaced. Nothing calls the module yet. For batch 2: `CodeTooLong`, `KeyTooLong` and
  `InvalidScope` become `400`; `InvalidPrincipal` and `InvalidContainer` mean the authenticator or
  the server is wrong and become `500`; `common::container` is removed so there is one way to build
  a container; the prefix builders above one environment (`isolated_workspace`, `isolated_env`,
  `shared_*`) are added when purge and the sweeper need them (slice 8).
