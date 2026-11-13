# RFC 0030: Remote cache trust model (tiers, immutability, provenance, signing)

- **Status:** Draft <!-- Draft | Accepted | Implemented | Superseded -->
- **Owner:** TBD
- **Date:** 2026-11-13
- **Base branch:** `main`
- **Risk tier:** High (caching and security boundary; see `AGENTS.md` risk overrides)
- **Affected crates/files:** `omni_remote_cache_core` (`security` module becomes the `Authenticator` seam and `Caller`, scopes, namespaces, trailer, `put_artifact`, `get_artifact`, `head_artifact`, provenance and purge routes, hooks, config); `omni_remote_cache_storage_{core,local,memory,s3,r2}` (atomic create-if-absent, range and prefix primitives, path safety, namespace-first layout, S3 key-prefix mode only); `omni_remote_cache_client` (including `validate_access`); `omni_remote_cache_repository_{query,sql,d1}` (`principal` column, hooks parameters); `omni_remote_cache_core/src/caching` (`CachedSecurity`); `omni_cache` (`cache/impls/hybrid.rs`, signing, verification, remote config, restore path validation); `omni_types` (cached path validation); `omni_configurations` (remote cache settings in the local file, `trusted_keys` and `verify` in `WorkspaceConfiguration`, task settings); `omni_cli_core` (`cache keygen`, `cache provenance`, `cache purge`); JSON schema; `apps/site` (remote cache guide, security page, release note)
- **Requirements:** none yet. Write `docs/requirements/0030-...` if this RFC is accepted.
- **Compatibility:** none required. Omni is pre-release and the built-in client is the only client, and we can update it in lockstep with the server. This RFC changes the protocol, storage layout, configuration and defaults freely. The server still enforces every rule, because the threat is a key holder who can send raw HTTP requests, not a well-behaved omni.

## 1. Summary

Today anyone who can write to a shared remote cache can plant an artifact that
other machines restore and run. Omni cannot tell who wrote an entry, lets any
writer overwrite any entry, uploads by default, and the client trusts whatever
it downloads.

This RFC makes the cache a trust boundary with six layered controls, each
covering a gap the others leave:

| # | Control | Closes |
| --- | --- | --- |
| A | **Push off by default** (client) | Accidental and casual uploads from developer machines. |
| B | **Immutable entries** (`409` on overwrite; only successful runs stored) | Overwriting a good entry, and the first-write race. |
| C | **Trust tiers.** `write:artifacts` writes an isolated namespace. Only `publish:artifacts` writes the shared one. | An untrusted PR or developer key poisoning what trusted builds restore (CREEP). |
| D | **Asymmetric signatures on shared entries**, verified by the client | A compromised or malicious cache server, storage, or a mis-scoped key. |
| E | **Per-task remote opt-out** | Release and deploy outputs ever coming from a cache. |
| F | **Server-authored provenance and purge** | Not knowing who wrote what, and having to wipe the whole cache after an incident. |

## 2. Motivation and current behavior

### Threat

A company runs one remote cache for all employees. A compromised key, a
malicious PR build, an insider, or a compromised cache server uploads or serves a
poisoned artifact. Other machines get a cache hit, restore the output, and run it.

The sharpest form is CREEP (CVE-2025-36852). An untrusted PR build produces a
poisoned artifact under the *same key* `main` will compute, because workflow files
are not part of the key, and wins the race to upload. Checksums and signatures
are valid, because the poisoning happens during construction. Only controlling
who can write to which namespace (C) stops it. Signing (D) does not, unless the
signing key is confined to trusted jobs. It is a second layer against server and
storage compromise, not a replacement for C.

### What the code does today (verified on `8293d547`)

- **Keys name inputs, not contents.** The client downloads by
  `bs58(project.digest)` (`hybrid.rs`). Content cannot be checked against it.
- **Scopes are flat.** `read:artifacts`, `write:artifacts`, `list:artifacts`,
  `delete:artifacts`, per API key (`ApiKeyConfiguration`), alongside a per-key
  environment list. A key with `write:artifacts` writes the one shared namespace.
- **Overwrite is allowed.** `LocalDiskCacheBackend::save_stream` truncates and
  rewrites. `hybrid.rs` deliberately overwrites a failed remote entry with a
  successful one (`replacing`).
- **Upload is on whenever remote is enabled.**
- **No provenance, no verification.** Hooks (RFC 0027) see an `api_key_id` in
  `before` and `after`. It is keyed only when `hooks.api_key_id_secret` is set in
  the SQL/D1 provider, and the static-config provider always uses the unkeyed
  16-hex SHA-256 prefix. Entries carry no writer information, and the client calls
  `unarchive` on whatever bytes it gets.
- **Extraction.** `tar` 0.4.46 `unpack` skips `..` entries and validates symlink
  destinations, so the archive layer is covered. The metadata inside the archive is
  not: restore resolves each cached `original_path` with `base.join(path)` and
  `std::path::absolute` (`omni_types/src/omni_path.rs`), which neither rejects `..`
  nor absolute paths. A hostile entry that gets past verification (or any entry
  when `verify` is `off`) could write outside the workspace on replay (4.1, path
  safety).
- **No remote retention.** Nothing in server config or storage expires entries. The
  only expiry today is an API key's `expires_at`.

### What other tools do (current docs, checked 2026-11-13)

| Tool | Approach |
| --- | --- |
| Nx Cloud | Read-only and read-write CI tokens. Read-only runs write to an isolated, branch-scoped cache that reads through to the shared one. Entries are immutable. OpenAPI spec requires `409` on existing keys. Optional end-to-end encryption. Its self-hosted bucket packages were deprecated (2026-05-21) as unfixable. |
| Gradle | Remote push off by default. CI populates, developers only read. Entries are not signed. Trust is the write-side ACL. Recommends disabling cache for release tasks. |
| Bazel | `--remote_upload_local_results=false`. Only CI should write. No way to tell which build wrote an entry, so recovery is wiping the cache. |
| moon | `localReadOnly`. Read-only and read-write tokens. |
| Turborepo | Opt-in HMAC-SHA256 signing with a shared secret. Failed verification is a miss. No write tiers. |
| Pants | `remote_cache_read`/`write` switches, no trust guidance. |
| Develocity | Trusted-writer isolation and provenance are on its roadmap. |

Every tool treats the write path as the control. No surveyed tool combines tiers,
immutability, asymmetric signatures and provenance. This RFC does.

## 3. Goals and boundaries

**Goals**

- A key without `publish:artifacts` cannot change what a trusted build restores.
- An untrusted PR build still gets cache hits from the shared cache and from its
  own earlier runs.
- A stored entry cannot be replaced by a writer.
- In `require` mode, a read-only run and any run that can publish restore only
  shared entries signed by a trusted publisher, even if the server and storage are
  fully compromised. Such runs never read an isolated namespace. Only a run that
  declares itself untrusted (`target: isolated`, and not able to publish) may also
  read its own isolated namespace, and nothing it builds can reach `shared`.
- An operator can tell who wrote an entry, and remove everything one key wrote
  without wiping the cache.
- A release task never touches the remote cache.
- A new credential type (OIDC token, user login) can be added by writing an
  authenticator, without changing routes, namespaces, provenance or hooks.

**Out of scope**

- A compromised publishing CI job that also holds the signing key. That is the
  trust root. Operators protect it with protected-branch-scoped secrets.
- Verifying that an isolated-namespace label belongs to the PR it names (needs VCS
  integration). The invariant held is that isolated entries never reach the shared
  namespace.
- Confidentiality (encryption of artifacts) and changing the task hash.
- Online key revocation service. Revocation is removing a key from `trusted_keys`.
- OIDC authentication and user login. They are later RFCs. This RFC only builds the
  seam they plug into (4.0).

## 4. Design

### 4.0 Authenticated caller (the identity seam)

Omni's server does not know *how* a caller proved who they are. Every route and
hook works from one authenticated value, so credentials other than API keys (OIDC
tokens for CI workloads, or user login) can be added later without changing
anything in this RFC:

```rust
struct Caller {
    principal: String,          // who owns the namespace (4.1); display form, <= 256 bytes
    credential: Credential,     // { kind: ApiKey | Oidc | ..., id: String }
    scopes: Scopes,             // effective scopes for the requested resource only
    scope_pin: Option<String>,  // a verified isolated-scope label, if the credential proves one
}

struct Resource { tenant: String, org: String, ws: String, env: String }   // from the request, validated first

trait Authenticator {           // replaces the bool-returning SecurityService methods
    /// Authenticates the credential for `resource` and returns a `Caller` whose `scopes`
    /// are the subset of `wanted` the credential holds there. `None` = not recognized.
    async fn authenticate(&self, credential: &str, resource: &Resource, wanted: Scopes)
        -> Result<Option<Caller>, AuthError>;
}
```

- The request carries one opaque credential (`Authorization: Bearer ...`). Today
  it travels in an `X-API-KEY` header and the `ApiKey` extractor reads it. The
  extractor becomes a `Credential` extractor reading `Authorization`, and the
  client changes in the same release (4.8.3). An authenticator
  chain tries each registered authenticator, and the first to recognize the
  credential returns the `Caller`. Today the chain has one member, the API-key
  authenticator (static config or SQL/D1 repositories, RFC 0027).
- Authentication is **per request and scoped to the target**: the route knows the
  resource (after `validate_ownership`) and the scopes it needs, and passes both, so
  the returned `Caller` is already limited to that resource and `Caller::can(scope)`
  is a synchronous check. This fits the SQL/D1 key contract, which is a set of
  per-request `exists` queries (`api_key_valid`, `_tenant`, `_organization`,
  `_workspace`, `_environment`, `_scope`) and cannot list what a key may access, and
  it fits the static-config provider. Routes ask only for the scope they need, so a
  SQL provider runs one `_scope` query per wanted scope, not five.
- `publish:artifacts` is never implied. `scopes: all` (which today matches every
  scope) grants everything **except** `publish`, which must be listed explicitly,
  so an existing `all` key does not silently become a publisher.
- `scope_pin` lets a future authenticator *prove* the isolated-scope label (for
  example a PR number from a verified CI token). When set, the server ignores the
  client's `scope`. API keys never set it.
- `credential.id` for API keys is `api_key_id` (RFC 0027): keyed when
  `hooks.api_key_id_secret` is set, unkeyed otherwise. This RFC makes the secret
  required wherever `credential.id` is exposed (see 4.3, provenance visibility).
  For other kinds it is a keyed hash of the issuer and subject. It is the only
  credential identifier that reaches hooks, provenance and logs.
- Three existing places must move with the seam: `CachedSecurity`
  (`caching/provider.rs`), which caches `bool` results per raw key and becomes a
  cache of `Caller` keyed by a hash of the credential **plus the `Resource` and the
  `wanted` scopes** (as `CachedSecurity::can_access` already includes
  `required_scopes` in its key), because a `Caller`'s `scopes` are only valid for the
  one resource and scope set it was built for; a hit never serves a `Caller` built
  for a different resource or wanted set; the `validate_access` call
  (`omni cache remote setup`), which also sends `X-API-KEY`; and the SQL/D1 key
  query contract, which gains an optional `principal` result column.
- Nothing downstream of the authenticator mentions API keys. Hooks get
  `caller_id` (`credential.id`) and `credential_kind` instead of `api_key_id`.

What this leaves for later, each its own RFC:

- **OIDC workload identity for CI.** An authenticator that validates a provider
  token (signature against the provider's published keys, issuer, audience, expiry)
  and maps verified claims to principal, scopes (for example `publish` only for a
  push to a protected branch) and `scope_pin`. It removes long-lived write secrets
  and the unauthenticated `scope` label. Needs a claim-mapping config format, key
  fetch and caching that also works on the Workers runtime, and per-provider claim
  checks.
- **User login.** `omni cache login` (device-code flow) that obtains a short-lived
  OIDC token for developers, and an authenticator that maps the user to a
  principal. Omni still does not own a user directory.

Both fit the seam above. The seam test in section 5 keeps that true.

### 4.1 Scopes and namespaces (control C)

Scopes (replace today's four):

| Scope | Allows |
| --- | --- |
| `read:artifacts` | `GET`/`HEAD` in the shared namespace and, if the key also has `write:artifacts`, its own isolated namespace. |
| `write:artifacts` | `PUT` into the caller's **isolated** namespace. |
| `publish:artifacts` | `PUT` into the **shared** namespace (implies `write`; never implied by `scopes: all`, see 4.0). |
| `list:artifacts` | List. |
| `delete:artifacts` | `DELETE` and purge in any namespace, for operators. |

A caller with only `read:artifacts` can never write.

Namespaces are storage containers. Today's container is `tenant/org/ws/env`. The
namespace comes **first**, so a bucket lifecycle rule can match one key prefix
(`isolated/`), which a middle path segment would not allow:

- shared: `shared/{tenant}/{org}/{ws}/{env}`
- isolated: `isolated/{tenant}/{org}/{ws}/{env}/{principal_id}/{scope}`

The S3 backend always maps a container to a key prefix inside one bucket. Today's
mode where the container is the bucket name cannot hold these slash-separated
containers and is removed (it is the `multi_bucket == false` branch of
`get_bucket_and_prefix`; the flag's name is inverted, since `true` means one bucket
with key prefixes).

`principal` (in the `Caller`, 4.0) identifies the *owner* of one or more
credentials (a user, service account or CI pipeline), not a credential. The
credential id is a hash of the credential, so two keys of one person would get
different namespaces. For API keys the operator assigns a `principal` where the
key is defined (an `ApiKeyConfiguration` field, or an optional `principal` result
column in the SQL/D1 key query contract, filled by the key issuer). It is never
taken from a request, so a caller cannot choose another principal's namespace. A
key with no `principal` defaults to its own `api_key_id`, so nothing is shared
unless the operator says so. Credentials that share a principal are trusted
equally in the isolated tier, so only group credentials of equal trust. Mapping a
verified identity to a principal is the authenticator's job (4.0), and the API-key
authenticator has no identity logic.

The namespace path segment is `principal_id`: the principal itself if it matches
`[A-Za-z0-9][A-Za-z0-9._-]{0,63}`, otherwise `h-` plus 32 hex characters of its SHA-256. This
keeps path-unsafe principals (future OIDC subjects such as
`repo:org/name:ref:refs/heads/x`) out of storage paths and keeps
the segment bounded. The readable principal is stored in the trailer (4.3).

`scope` is a client-chosen label (default derived from the CI environment, see
4.4), restricted to `[A-Za-z0-9][A-Za-z0-9._-]{0,63}`, unless the `Caller` carries a
`scope_pin`, which wins. One principal can separate branches or PRs, and
distributed agents of one CI run share a namespace by sending the same `scope`.

**Path safety.** Namespaces and digests become storage paths, so they are validated
at the boundary, not trusted:

- `scope` and `principal_id` match `[A-Za-z0-9][A-Za-z0-9._-]{0,63}`. The first
  character must be alphanumeric, so `.` and `..` cannot occur.
- `{digest}` must be base58 of bounded length (validated in every artifact route,
  before any path is built). Today the path parameter is unvalidated, and a
  percent-decoded value such as `..%2F..%2Fshared%2FX` from an isolated writer would
  resolve into `shared/` on the local backend.
- Every backend that maps keys to filesystem paths (`omni_remote_cache_storage_local`)
  checks that the resolved path stays under the container root, and fails closed.
- Prefix operations use namespace-terminated prefixes
  (`isolated/{t}/{o}/{w}/{e}/{p}/`), because a prefix without the trailing `/` also
  matches `{p}2`.
- Restore validates each cached `original_path` from the entry metadata: it must be
  relative, contain no `..`, and stay inside the project root, or the entry is a
  miss. This applies to every restore, whatever the namespace or `verify` mode.

The default principal for a key with no configured `principal` is its `api_key_id`,
so changing `hooks.api_key_id_secret` changes every default principal and orphans
their isolated namespaces. Operators who need stable namespaces set explicit
principals.

Resolution is **client-driven and explicit**, so the client never trusts the
server's claim about where a hit came from:

- `GET`/`HEAD` take `?namespace=shared|isolated` (default `shared`).
- **When the client probes `isolated`.** Only when all of these hold: its configured
  `target` is `isolated`; the credential has `write:artifacts` and does **not** have
  `publish:artifacts` (the client learns its effective scopes from `validate_access`,
  4.4); and it then probes `isolated` and `shared` in parallel and prefers its own
  isolated entry. A read-only client, a publisher, and any run with `target: shared`
  probe `shared` only. So trusted runs and verifying readers never read unsigned
  isolated bytes, even if a compromised server answers the probe.
- `PUT` takes `?target=shared|isolated` (default `isolated`). `shared` needs
  `publish:artifacts`, otherwise `403`.
- An isolated entry is never copied or promoted into shared.
- If the isolated probe fails (for example a `5xx`), the client falls back to
  `shared`, and never fails the run because of it.
- When the same digest exists in both namespaces, the client's own probe order
  decides (isolated first). Nothing on the server merges them.

### 4.2 Immutability (control B)

- `PUT` to an existing entry in the target namespace returns `409 Conflict`. No
  replace exists. Storage backends add an atomic create-if-absent. A
  check-then-write race is not acceptable, because the race is the attack.
  - local: write a temp file, then link/rename without replacing.
  - memory: entry API.
  - S3: single `PutObject` with `If-None-Match: *` (AWS documents `412` when the
    key exists, and `409 ConditionalRequestConflict` on a racing delete, which the
    server retries). `CompleteMultipartUpload` supports the same header.
  - R2: R2's S3 API documents atomic `If-None-Match: *` on `PutObject` and
    `CompleteMultipartUpload`. The Workers binding documents `onlyIf` on `put`
    (returns `null` when the condition fails), but does not document `*`, and a
    2024 report (workers-sdk #6411, Miniflare) saw `etagDoesNotMatch: "*"`
    behave the wrong way round. The R2 backend therefore must not rely on it until
    the spike proves create-only behavior on **real R2**, not only Miniflare. If it
    fails, the R2 backend uses the S3 API (SigV4 `PutObject` with `If-None-Match: *`).
  - **Size cap.** The server config `max_artifact_size` bounds an entry
    (default 256 MiB on native servers, 32 MiB on Workers: an isolate has 128 MB of
    memory, and the entry is buffered to hash it and append the trailer), so every backend commits with one
    conditional single-object write. `omni_remote_cache_storage_r2` uses the
    Workers binding (`worker` 0.8.7), whose multipart `complete` takes no
    condition, so multipart must not be on the commit path. The client skips
    uploading artifacts over the cap and reports a diagnostic.
- `DELETE` is operator-only (`delete:artifacts`).
- **New storage primitives.** The trait has no range read and no prefix operation
  today (`omni_remote_cache_storage_core/src/traits.rs`), and the design needs both:
  `get_range(Suffix(n) | Bounded(a, b))` (S3 `Range: bytes=-n`, R2 `Range::Suffix`,
  trivial on local and memory), a recursive, paginated `list_prefix` (S3 `list` is
  unpaginated `list_objects` capped at 1000 keys, and the local `list` is
  non-recursive), and `delete_prefix`. `lru_cached` implements them and invalidates
  on purge.
- **Truncated uploads must not commit.** Today `put_artifact` swallows upstream
  stream errors (`filter_map(... Err => None)`), so a cut-off upload is committed as
  if complete. The sealing stream (4.3) yields `Err` on any upstream error or digest
  mismatch, and every backend aborts on `Err`. The shared storage contract test
  covers it.
- The client treats `409` as success ("someone already stored it").
- **Only successful executions are stored remotely.** Remove the `replacing` path
  in `hybrid.rs`. Failed results stay local. Immutability then has no exception.

### 4.3 Entry trailer and provenance (control F)

Each remote entry is one object, so the artifact and its metadata commit
atomically (no state where one exists without the other). The metadata is a
**trailer**, so the server writes in a single pass: it spools the body while
hashing it, then appends the trailer once the hash is known:

```
[artifact: tar.gz bytes][trailer: msgpack][u32 trailer_len][magic "OMNIENT1"]
```

Readers find the trailer with a suffix range read (the last 64 KiB), so operator
scans (4.3 purge) read a small tail instead of whole artifacts. Trailer
(`trailer_len` bounded to 64 KiB):

| Field | Source |
| --- | --- |
| `version` | server |
| `writer_credential` (`kind`, `id`), `writer_principal` | **server** (from the authenticated `Caller`) |
| `namespace`, `scope` | **server** (from the validated request) |
| `stored_at` | **server** |
| `size`, `sha256` | **server** (computed while streaming; must equal the client-declared `X-Omni-Content-Sha256` or the upload is rejected and nothing is committed) |
| `signature` | client-supplied, opaque to the server (4.5) |

The provenance fields are never taken from the client, so a writer cannot forge
who wrote an entry. The server commits the object only after the stream completes and
the digest matches. Core wraps the request body in a **sealing stream** that hashes
as it passes bytes, enforces `max_artifact_size`, appends the trailer at EOF, and
yields `Err` on an upstream error or a digest mismatch. Backends stay responsible
for spooling (S3 already spools to a temp file, and Workers buffer in memory) and
perform the conditional single write from 4.2, aborting on `Err`. This needs no
filesystem in core, so it works on native and wasm32. A mismatch commits nothing and
leaves the key free. On Workers the peak memory is about twice the entry (body plus
sealed copy), so the 32 MiB cap means roughly 64 MiB of the 128 MB isolate.

`GET` reads the trailer with a suffix range read, then streams the body range
`[0, size - trailer_len - 12)` (the trailer, its length and the magic are not
returned). It sets `X-Omni-Namespace`, `X-Omni-Stored-At`, `X-Omni-Content-Sha256`
and `X-Omni-Signature` (`<key_id>:<base64>`, bounded to 256 bytes). It sets
`X-Omni-Credential` and `X-Omni-Principal` **only for callers with `list:artifacts`
or `delete:artifacts`**: read-only clients do not need them (trust comes from the
signature, 4.5), and an unkeyed credential id would let them test key guesses
offline and learn org structure.

Operator tooling:

- `omni cache provenance <digest>` needs `list:artifacts` and calls
  `GET /v1/artifacts/{digest}/provenance?namespace=...`.
- `omni cache purge` needs `delete:artifacts` (not `list`) and calls
  `DELETE /v1/artifacts?namespace=...&(principal|credential|signing_key)=...&since=...`:
  `omni cache purge (--credential <id> | --principal <id> | --signing-key <key_id>) [--since <time>] [--namespace ...]`.
  `--signing-key` removes entries whose trailer names that signing key, and
  `--legacy` removes the pre-RFC container layout.
  Purging a **principal's isolated namespace** is a prefix delete
  (`isolated/{t}/{o}/{w}/{e}/{principal_id}/`, repeated for each environment of the
  workspace the server knows), with no scan. Purging by `--credential`, or purging the
  **shared** namespace, scans: the server lists the container, reads each trailer
  with a suffix range read, and deletes matches. Scans are for incidents, and
  `--since` and `--namespace` narrow them. An index is added only if measurements
  show scans are too slow.

Hooks (`HookContext`) replace `api_key_id` with `caller_id` and `credential_kind`, and gain
`namespace`, `scope`, `principal` and, on `after`, `sha256`, so audit logs record the same facts.
`OutcomeKind` gains `AlreadyExists`, because `after_response` today maps every non-2xx
to `Failed`, which would log the normal `409` as a failure. The SQL hooks contract
(RFC 0027, `repository_query/src/hooks.rs`) binds positional parameters, 7 for
`before` (`op, tenant, org, ws, env, digest, api_key_id`) and 8 for `after`. The new
order is `before`: `op, tenant, org, ws, env, digest, caller_id, credential_kind,
principal, namespace, scope` (11), and `after` adds `outcome, sha256` (13). The
`validate_queries` count checks are updated and operators rewrite their SQL.

### 4.4 Client configuration (controls A and E)

Verification policy is **workspace policy**, so it lives in the committed workspace
configuration (`workspace.omni.yaml`, `WorkspaceConfiguration` in
`omni_configurations`) and applies on every machine that checks the repository out.
Credentials and the signing key are secrets, so they stay in the machine-local
remote cache file (`<cache_dir>/remote-cache.omni.{yaml,enc}`,
`omni_configurations/src/remote_cache_configuration.rs`, `deny_unknown_fields`).

```yaml
# workspace.omni.yaml (committed)
remote_cache:
  signing:
    trusted_keys:      # public keys, safe to commit
      - id: ci-2026-11
        public_key: "<base64 ed25519>"
    verify: auto       # auto | off | warn | require (see 4.5)
```

```yaml
# <cache_dir>/remote-cache.omni.yaml (machine-local, may be encrypted)
push: false            # default. true uploads successful results on a miss
target: isolated       # isolated | shared (shared needs publish:artifacts)
scope: auto            # auto derives from CI env (PR number, branch); or a literal
signing_key:           # publishers only; never committed
  id: ci-2026-11
  private_key_env: OMNI_CACHE_SIGNING_KEY
```

Local config may **tighten** the workspace policy but never loosen it. Resolution
order: first resolve the workspace `verify` (`auto` becomes `require` when the
**workspace** `trusted_keys` is non-empty, otherwise `off`); then take the stricter of
that and the local `verify` (`off` < `warn` < `require`); then apply any local
narrowing of `trusted_keys`. `auto` is never re-evaluated after narrowing, so a local
file cannot lower the policy by narrowing the key set to nothing. The effective
`trusted_keys` is the workspace set, which a local file may only narrow to a subset
(a release pipeline can trust only the release publisher's key). An empty effective
key set under `require` rejects every shared entry as a miss. A local file cannot
add a key or turn verification down, so a machine nobody configured still verifies
whatever the repository says.

- `push: false` is the default, so developer machines only read, and a key with
  only `read:artifacts` makes no upload attempts. The server enforces this
  regardless (the flag prevents normal uploads, and the server token prevents a
  changed config or a malicious build step). With tiers, `403` and `413` become
  normal results (for example `push: true` with a read-only key), so an upload
  failure is a warning and never fails the run (today any upload error does,
  `hybrid.rs` propagates the join result). The server advertises its
  `max_artifact_size` through `validate_access`, and the client uses the smaller of
  that and its own setting, for uploads and for downloads (today `get_artifact`
  buffers an unbounded `bytes()`). `validate_access` also returns the credential's
  effective scopes for the workspace, which is how the client knows whether it can
  publish (4.1).
- `scope: auto` is `pr-<number>` for a pull request, else the branch name (made
  safe for `[A-Za-z0-9][A-Za-z0-9._-]{0,63}`, hashed if too long), else `default`. Detection
  (checked in this order):

  | CI | Detected by | PR number | Branch |
  | --- | --- | --- | --- |
  | GitHub Actions | `GITHUB_ACTIONS` | `GITHUB_REF` (`refs/pull/N/merge`) | `GITHUB_HEAD_REF`, else `GITHUB_REF_NAME` |
  | GitLab CI | `GITLAB_CI` | `CI_MERGE_REQUEST_IID` | `CI_COMMIT_REF_SLUG` |
  | CircleCI | `CIRCLECI` | `CIRCLE_PULL_REQUEST` (trailing number) | `CIRCLE_BRANCH` |
  | Buildkite | `BUILDKITE` | `BUILDKITE_PULL_REQUEST` (if not `false`) | `BUILDKITE_BRANCH` |
  | Jenkins | `JENKINS_URL` | `CHANGE_ID` | `BRANCH_NAME` |

  The names are from CI provider docs as I know them, to be re-checked when the
  detector is written. Omni has no CI detection today. `scope: run` (set
  explicitly) uses the provider's run id for a namespace no other run shares.
  Labels are not authenticated: all PR jobs using one principal share a trust tier,
  so one PR could write another PR's label. That cannot reach the shared
  namespace, and operators who run fork PRs should give them a read-only key.
- **Per-task opt-out (E):** a task sets `cache.remote: false` (inside the existing
  `CacheConfiguration`, not a top-level `remote_cache`, which would collide with the
  client config), and the CLI offers
  `--no-remote-cache`. The task neither reads nor writes remotely. Release, deploy
  and signing tasks set it. **It applies to the task and all its transitive
  dependencies in that run**, because a release that depends on a `build` restored
  from the remote cache would still ship a poisoned output. A local cache entry
  restored from the remote cache gets a marker file (`.omni-origin-remote`) in its
  directory. The marker records the origin namespace, `shared` or `isolated`. An
  opted-out run treats **every** marked entry as a miss. Any other run treats an
  entry marked `isolated` as a miss, unless the run is untrusted, meaning its
  credential has `write:artifacts` but not `publish:artifacts` (the client knows this
  from `validate_access`; if the scopes are unknown, for example remote is disabled or
  unreachable, the run is treated as trusted, the strict case). The trust tier comes
  from the credential's scopes, not from the configured `target`, because `target`
  defaults to `isolated` and does not identify untrusted runs. Entries marked
  `shared` were verified at restore time and are reused by any run, so local hits are
  not lost. Unmarked entries are used as usual. This stops an unsigned isolated entry,
  restored earlier into a shared local cache directory (for example a CI cache
  restored between jobs), from reaching a shared publish through a local hit. When a
  run that must reject a marked entry re-executes the task, a
  local success **replaces** the marked entry (`publish_staged_dir(.., replace =
  true)`), otherwise `cache_many` sees `existing_exit_code == Some(0)`, writes
  nothing, and every such run rebuilds forever. The marker is a separate file, not a field in the entry
  metadata, so the metadata format and older readers are unaffected. The client
  extracts into a sibling temp directory, writes the marker first, then renames the
  directory into place. A crash can then never leave a remote entry without its
  marker (it also fixes partial extraction, which today unpacks straight into the
  final directory). Transitive semantics need a reverse-reachability pass over the
  whole plan before the first batch (today's demotion pass in `cache_status.rs` and
  `batch_executor.rs` runs the other direction), and a per-item `remote` flag passed
  to `get_many` and `cache_many`, because remote config is one value per store today.
  The flag stays out of `TaskExecutionInfo` serialization, which feeds hashing, so
  digests do not change.

### 4.5 Asymmetric signatures (control D)

**Who signs.** Only uploads with `target: shared` are signed. Isolated entries are
unsigned, since they are the caller's own untrusted-tier writes. A shared upload
without a configured `signing_key` is refused client-side when any `trusted_keys`
are configured, so a misconfigured publisher fails loudly.

**Algorithm.** Ed25519 (`ed25519-dalek`). CI holds the private key. Clients hold
only public keys, so verifiers cannot forge (unlike a shared HMAC secret).

**Signed message** (each field length-prefixed with a big-endian `u32`, so the
encoding is unambiguous whatever bytes the codes contain) binds the entry to its place, so it cannot be replayed under a
different key, tenant or namespace:

```
len||"omni-cache-v1" ‖ len||tenant ‖ len||org ‖ len||ws ‖ len||env
  ‖ len||"shared" ‖ len||digest ‖ len||sha256(archive_bytes)
```

The signature stored in the trailer is `{key_id, signature}`, and on the wire (upload
header and `GET` response header `X-Omni-Signature`) it is `<key_id>:<base64
signature>`, bounded to 256 bytes.

**Verification (client, shared hits only).** Before `unarchive`: recompute
`sha256` of the downloaded archive bytes, rebuild the message, find
`key_id` in `trusted_keys`, and verify. Any failure (unsigned, unknown key id, bad
signature, hash mismatch) is treated as a miss, with a diagnostic naming the
digest and reason. The task then runs locally. Isolated hits are not verified,
because isolated entries are unsigned by design, so the rule is on *who may read
them* (4.1): only an untrusted run (`target: isolated`, not able to publish) ever
reads an isolated namespace. Trusted runs and read-only runs never do, so a
compromised server cannot feed them unsigned bytes through the isolated probe.

**Modes.**

- `off`: no verification.
- `warn`: verify, report failures as diagnostics, but still restore.
- `require`: failures are misses.
- `auto` (default): `require` if `trusted_keys` is non-empty, otherwise `off`.
  Adding a trusted key is what turns enforcement on, so there is no separate
  switch to forget.

**Keys.** `omni cache keygen` prints a new keypair and key id (first 16 hex chars
of SHA-256 of the public key). The private key is read from the environment or a
file, never from committed config, and is registered with log redaction
(RFC 0022 and 0026). Rotation: trust both old and new public keys, switch the
publisher, then remove the old key. Entries signed by a removed key become misses,
but they cannot be re-published, because the key is occupied (immutability, 4.2).
Rotation therefore ends with `omni cache purge --signing-key <old id>`, after which
the next publisher run re-publishes. The same purge-then-republish applies to an
entry found tampered or unverifiable.

**What this adds beyond C.** With the signing key confined to the publish job, a
client in `require` mode does not trust the cache server or storage: a server that
serves a different artifact, a mis-scoped key that gains `publish`, or someone with
raw storage access cannot produce an entry that verifies. For release pipelines,
set `trusted_keys` to only the release publisher's key, so builds accept
nothing else.

### 4.6 Lifecycle

Isolated entries expire by age since `stored_at` (default 7 days), so branches and
PRs do not accumulate storage. Entries are immutable, so there is no "last write"
to refresh. For S3 and R2, operators use one bucket lifecycle rule on the
`isolated/` key prefix (possible because the namespace is the first path segment, 4.1),
and the guide ships the rule, so no server job is needed. The
local and memory backends run a server sweeper. The setting is server-wide. Shared
entries never expire (nothing in the server or storage expires them today), so operators purge
them or set their own bucket rules.

`list:artifacts` takes `?namespace=shared|isolated` like get and head (default
`shared`), and `isolated` lists only the caller's own principal.

### 4.7 Worked example (test fixture)

1. Keys, each with an explicit `principal` so paths are readable: `ci-main`
   (`read`, `write`, `publish`; principal `ci-main`), `ci-pr` (`read`, `write`;
   principal `ci-pr`), `dev` (`read`; principal `dev`). `ci-main` also holds signing key `K_ci`. The workspace config lists
   `K_ci`'s public key in `trusted_keys` (so `verify: auto` means `require`).
2. `ci-main` builds `app#build` with `target: shared`, signs, and uploads digest
   `D`. The server records the writer credential of `ci-main`, principal `ci-main`,
   namespace `shared`.
3. `dev` builds `app#build`. It is read-only, so it probes `shared` only, verifies
   the signature, restores `D`, and uploads nothing (`push: false`). A forced `PUT`
   returns `403`.
4. A PR runs as `ci-pr` (`target: isolated`, no `publish`), `scope: pr-17`, with a changed workflow that produces a
   poisoned artifact for the same digest `D2` that `main` will compute. It uploads
   to `isolated/ci-pr/pr-17`. A later `pr-17` job hits it.
5. `ci-main` builds `D2` on `main`. It publishes, so it probes `shared` only, never
   an isolated namespace. It is a miss, so it builds and publishes. The poisoned
   entry is never used.
6. `ci-pr` tries `PUT ?target=shared`: `403`. `ci-main` tries to re-`PUT` `D`:
   `409`.
7. **Server compromise:** an attacker with raw storage access replaces the bytes
   of `D` in `shared`. `dev` verifies, the hash and signature fail, and the entry
   is a miss. The task runs locally with a diagnostic. The entry stays occupied, so
   the operator purges it (`omni cache purge --signing-key ...` or by digest) and the
   next publisher run re-publishes.
8. **Incident:** key `ci-pr` is found compromised. The operator runs
   `omni cache purge --credential <ci-pr id>`. Only that credential's entries go (`--principal` removes
   everything its owner wrote, across keys), and the
   shared cache is untouched.
9. A release task has `cache.remote: false` and rebuilds from source regardless.

Edge cases the fixture must cover:

- Two concurrent `PUT`s to the same key: exactly one wins, the other gets `409`.
- Declared `X-Omni-Content-Sha256` does not match the streamed bytes: rejected,
  nothing committed, key stays free.
- A `scope` with `/`, `.`, `..`, a leading dot, empty, or over 64 characters:
  rejected. A `{digest}` that is not bounded base58, including `..%2F..%2Fshared%2FX`:
  rejected before any path is built. A resolved path that leaves the container root:
  rejected. No request field can select another principal's namespace.
- A cached `original_path` that is absolute, contains `..`, or leaves the project
  root: the entry is a miss, in every namespace and `verify` mode.
- A truncated upload (upstream stream error): nothing is committed and the key stays
  free.
- The isolated probe returns `5xx` while shared hits: the client uses shared.
- A publisher gets `409` on a shared upload: that is success, but the entry that
  exists is not proof it is signed or valid. A later `require`-mode reader decides.
- Two keys with the same `principal` share one isolated namespace. A key with no
  `principal` gets its own. Rotating a key keeps the principal's namespace.
- A client-supplied provenance field in the upload is ignored (writer, namespace,
  time are server-derived).
- Signature valid for another digest, namespace, env, tenant or archive hash:
  rejected. Unknown or removed `key_id`: rejected. Both old and new keys trusted
  during rotation: both accepted.
- Missing or truncated trailer, oversized `trailer_len`, bad magic: miss, never a
  panic.
- `verify: warn` restores but reports (the verdict is `Warn`, not `Reject`). `auto` with no trusted keys behaves as
  `off`.
- A publisher with `trusted_keys` set but no `signing_key` refuses to upload.
- Isolated and shared entries for the same digest: an untrusted run prefers its own
  isolated entry by its own probe order. Pin it.
- A publisher or a read-only run, with a compromised server answering the isolated
  probe: the client never sends it, so nothing unsigned is restored. A publisher
  configured with `target: isolated` is still treated as a publisher (it has
  `publish`) and does not probe isolated.
- A local config that sets `verify: off` or adds a `trusted_keys` entry that the
  workspace config does not list: ignored, and the stricter workspace policy applies.
- A key with `scopes: all`: can read and write isolated, and gets `403` on a shared
  `PUT`.
- TTL expiry removes isolated entries only.
- Archive with `..` entries and escaping symlinks stays skipped (pin test).
- Failed execution is never uploaded.

### 4.8 Code changes: before, after, why

"Before" snippets are today's code on `8293d547`, trimmed. "After" snippets are
sketches of the intended shape, not final code. Each change ends with its reason.

#### 4.8.1 Storage: overwrite becomes create-if-absent (B)

Before, `omni_remote_cache_storage_local` truncates whatever is there:

```rust
async fn save_stream(&self, container: Option<&str>, key: &str,
                     mut value: BoxStream<Bytes>) -> Result<(), Error> {
    let path = self.path(key, container);
    // ... create_dir_all ...
    let mut file = OpenOptions::new()
        .create(true).write(true).truncate(true)   // overwrites an existing entry
        .open(&path).await.map_err(Error::custom)?;
    while let Some(chunk) = value.next().await { file.write_all(&chunk).await?; }
    Ok(())
}
```

After, a new trait method on `RemoteCacheStorageBackend` commits atomically and
reports whether it won:

```rust
pub enum SaveOutcome { Stored, AlreadyExists }

/// Commits `value` only if `key` does not exist. Never replaces. Writes to a temp
/// object first and commits with one conditional operation, so a partial upload is
/// never visible and two racing writers cannot both win.
async fn save_stream_if_absent(&self, container: Option<&str>, key: &str,
                               value: BoxStream<Bytes>) -> Result<SaveOutcome, Error>;

// local: write temp file in the same dir, then hard_link(temp, path); EEXIST => AlreadyExists
// memory: HashMap entry API
// s3:     PutObject with If-None-Match: * ; 412 => AlreadyExists ; 409 => retry
// r2:     put(..).only_if(etag_does_not_match "*") ; None => AlreadyExists (open question 1)
```

`save_stream` (replacing) is removed from the trait, so no caller can overwrite by
accident. Why: today's `save_stream` is the overwrite the attacker needs, and a
check-then-write helper would still race. The S3 backend already spools to a
temp file before `put_object`, so the S3 change is the header plus the `412` mapping.

#### 4.8.2 Remote upload: stop replacing, stop uploading failures (A, B)

Before, `hybrid.rs` uploads every result, and a failed remote entry is replaced:

```rust
if self.remote_config.is_enabled() {
    cached_results.push((execution_hash, output_dir.to_path_buf(), replacing));
}
...
// A remote artifact is keyed by digest, so an existing one would carry the
// superseded (failed) execution. Skip the existence check when replacing so the
// successful entry overwrites it; otherwise avoid a redundant re-upload.
if !replacing && client.artifact_exists(&config, &digest).await? { return Ok(()); }
archive(&output_dir, Cursor::new(&mut artifact))?;
client.put_artifact(&config, &digest, Bytes::from(artifact)).await?;
```

After:

```rust
if conf.push && new_cache_info.exit_code == 0 {            // A: opt in. B: successes only
    cached_results.push((execution_hash, output_dir.to_path_buf()));   // no `replacing`
}
...
archive(&output_dir, Cursor::new(&mut artifact))?;
if artifact.len() as u64 > conf.max_artifact_size { tracing::warn!(...); return Ok(()); }
match client.put_artifact(&config, &digest, Bytes::from(artifact), sig).await {
    Ok(PutOutcome::Stored | PutOutcome::AlreadyExists) => Ok(()),   // 409 is success
    Err(e) => { tracing::warn!(%digest, error = ?e, "remote upload failed"); Ok(()) }  // 403/413/5xx never fail the run
}
```

The `artifact_exists` pre-check stays only as an optimization. Local replacement of a
failed local entry by a success (`publish_staged_dir(.., replace)`) is unchanged: it
is a local cache concern and not a trust boundary. Why: the remote only ever holds
successes, so nothing needs replacing, and immutability has no exception.

#### 4.8.3 Authentication: from a bool per call to an authenticated `Caller` (C, 4.0)

Before, the credential is one header, and every route asks the security service a
yes or no question:

```rust
pub struct ApiKey(pub String);                       // reads header "X-API-KEY"

#[async_trait]
pub trait SecurityService {
    async fn can_access(&self, api_key: &str, tenant_code: &str, organization_code: &str,
        workspace_code: &str, environment_code: &str, required_scopes: &[&str])
        -> Result<bool, SecurityServiceError>;
    // ... can_access_tenant / _organization / _workspace / _environment, is_valid
}

// put_artifact.rs
guard!(state.provider, &api_key, &tenant_code, &query, &["write:artifacts"]);
```

After:

```rust
pub struct Credential(pub String);                   // Authorization: Bearer <token>

pub struct Resource { tenant: String, org: String, ws: String, env: String }
pub struct Caller { principal: String, credential: CredentialId,
                    scopes: Scopes,            // effective scopes for this resource only
                    scope_pin: Option<String> }
impl Caller { pub fn can(&self, scope: Scope) -> bool; }   // synchronous

#[async_trait]
pub trait Authenticator {
    async fn authenticate(&self, credential: &str, resource: &Resource, wanted: Scopes)
        -> Result<Option<Caller>, AuthError>;
}

// put_artifact.rs
let target = query.target.unwrap_or(Tier::Isolated);
let needed = match target { Tier::Shared => Scope::Publish, Tier::Isolated => Scope::Write };
let resource = Resource::new(&tenant_code, &query.org, &query.ws, &query.env);
let caller = authenticate!(state, credential, &resource, needed.into())?;   // 401 if none
if !caller.can(needed) { return forbidden(); }
```

The credential moves from `X-API-KEY` to `Authorization: Bearer`, because that is
where future OIDC tokens belong. Only the built-in client sends it, so it changes in
the same release. Why: `principal`, scope pinning and a second credential type
cannot be expressed through boolean `can_*` methods that take a raw API key.

#### 4.8.4 Server `PUT`: namespaces, trailer, `409` (B, C, F)

Before, one route writes to the one shared container:

```rust
let container = container(&tenant_code, &query.org, &query.ws, &query.env);
let x = state.storage_backend
    .save_stream(Some(container.as_ref()), &digest, Box::pin(stream))
    .await.map_err(InternalServerError);
```

After:

```rust
let ns = Namespace::resolve(&caller, target, query.scope.as_deref())?;  // shared | isolated/.../{principal_id}/{scope}
let container = container(&tenant_code, &query.org, &query.ws, &query.env, &ns);
let declared = header_sha256(&parts)?;                                   // X-Omni-Content-Sha256
let entry = SealingStream::new(stream, declared, trailer_from(&caller, &ns), max_artifact_size);
//   hashes as bytes pass, enforces the cap, appends the trailer at EOF, and yields Err on an
//   upstream error or a digest mismatch; the backend spools and aborts on Err (4.2, 4.3)
match state.storage_backend.save_stream_if_absent(Some(container.as_ref()), &digest, entry).await? {
    SaveOutcome::Stored        => StatusCode::NO_CONTENT,
    SaveOutcome::AlreadyExists => StatusCode::CONFLICT,
}
```

Why: the namespace and trailer fields come from the authenticated `Caller`, never
from the request, so a writer cannot forge who wrote an entry or choose another
principal's namespace.

#### 4.8.5 Client restore: verify, then extract atomically (D, 4.4)

Before, downloaded bytes go straight into the final directory:

```rust
if let Some(bytes) = response {
    unarchive(&output_dir, Cursor::new(bytes))?;      // no verification; partial on crash
}
```

After:

```rust
let Some(entry) = client.get_artifact(&access, &digest, Namespace::Shared).await? else { return Ok(()) };
match verify_shared(&conf.signing, &access, &digest, &entry) {
    Verdict::Ok => {
        let staging = staging_dir_for(&output_dir);
        unarchive(&staging, Cursor::new(entry.archive))?;
        validate_metadata_paths(&staging, &project_root)?;       // relative, no `..`, inside root
        write_origin_marker(&staging)?;                         // .omni-origin-remote
        publish_staged_dir(&staging, &output_dir, &metadata_path, false).await?;
    }
    Verdict::Warn(why) => { tracing::warn!(%digest, %why, "remote entry unverified"); /* restore as Ok */ }
    Verdict::Reject(why) => { tracing::warn!(%digest, %why, "remote entry rejected"); /* miss */ }
}
// Probe order (4.1): isolated is probed only when target is isolated AND the credential
// has write but not publish; then shared. An isolated probe error falls back to shared.
```

Why: verification must run before any byte reaches disk, and the temp directory plus
rename reuses `publish_staged_dir`, which the local write path already uses, so a crash
leaves neither a half-extracted entry nor an unmarked remote one.

#### 4.8.6 Client upload call (A, B, D)

Before, `RemoteAccessArgs` has no namespace, and any non-2xx is an error:

```rust
pub struct RemoteAccessArgs<'a> { api_base_url, api_key, tenant, org, ws, env }

let response = self.client.put(url)
    .header("X-API-KEY", remote.api_key).header("X-OMNI-TENANT", remote.tenant)
    .body(artifact).send().await?;
if response.status().is_success() { Ok(()) } else { Err(/* status code */) }
```

After:

```rust
pub struct RemoteAccessArgs<'a> { /* as before, minus api_key */ credential: &'a str,
                                  tier: Tier, scope: &'a str }

let response = self.client.put(url_with(tier, scope))
    .bearer_auth(remote.credential).header("X-OMNI-TENANT", remote.tenant)
    .header("X-Omni-Content-Sha256", sha256_hex)
    .header("X-Omni-Signature", signature)       // shared uploads only: "<key_id>:<base64>"
    .body(artifact).send().await?;
match response.status() {
    s if s.is_success()           => Ok(PutOutcome::Stored),
    StatusCode::CONFLICT          => Ok(PutOutcome::AlreadyExists),
    s                             => Err(/* status code */),
}
```

Why: `409` is the normal result of two builds racing, not a failure.

#### 4.8.7 Hooks: `api_key_id` becomes caller facts (F, 4.0)

Before:

```rust
pub struct HookContext { pub op: ArtifactOp, pub tenant: String, pub org: String,
    pub ws: String, pub env: String, pub digests: Vec<String>,
    pub api_key_id: String }
```

After:

```rust
pub struct HookContext { /* as before, minus api_key_id */
    pub caller_id: String, pub credential_kind: CredentialKind, pub principal: String,
    pub namespace: NamespaceKind, pub scope: Option<String> }
```

Why: hooks and audit logs record the same facts as the trailer, and nothing downstream
assumes an API key.

#### 4.8.8 Signing: new code, no before (D, 4.5)

```rust
fn signed_message(t: &str, o: &str, w: &str, e: &str, digest: &str, sha256: &[u8; 32]) -> Vec<u8> {
    let mut m = Vec::new();
    for f in [&b"omni-cache-v1"[..], t.as_bytes(), o.as_bytes(), w.as_bytes(), e.as_bytes(),
              &b"shared"[..], digest.as_bytes(), &sha256[..]] {
        m.extend_from_slice(&(f.len() as u32).to_be_bytes());   // length prefix: no ambiguity
        m.extend_from_slice(f);
    }
    m
}
fn verify_shared(cfg: &SigningConfig, a: &RemoteAccessArgs, digest: &str, entry: &Entry) -> Verdict {
    // `Warn` mode downgrades every Reject below to Verdict::Warn.
    if cfg.mode() == Mode::Off { return Verdict::Ok }
    if sha256(&entry.archive) != entry.trailer.sha256 { return Verdict::Reject("hash") }
    let Some(sig) = &entry.trailer.signature else { return Verdict::Reject("unsigned") };
    let Some(key) = cfg.trusted_keys.get(&sig.key_id) else { return Verdict::Reject("unknown key") };
    key.verify(&signed_message(a.tenant, a.org, a.ws, a.env, digest, &entry.trailer.sha256), &sig.bytes)
       .map_or(Verdict::Reject("bad signature"), |_| Verdict::Ok)
}
```

Why: the message binds the entry to its place and content, and only the public key is
needed to verify, so a client that verifies cannot forge.

#### 4.8.9 Client config (A)

Before, `EnabledRemoteConfig` has the key and codes only and always uploads:

```rust
pub struct EnabledRemoteConfig { api_base_url, api_key, tenant_code,
    organization_code, workspace_code, environment_code: Option<String> }
```

After, it gains the trust settings from 4.4 (`api_key` is renamed `credential`). The
verification policy comes from the committed workspace config merged with the local
file, never loosened by it:

```rust
pub struct EnabledRemoteConfig { /* as before, api_key -> credential */
    pub push: bool,                        // default false (machine-local)
    pub tier: Tier,                        // default Isolated (machine-local)
    pub scope: ScopeConfig,                // Auto | Run | Literal(String) (machine-local)
    pub signing_key: Option<SigningKey>,   // publishers only (machine-local, secret)
    pub verify: VerifyPolicy,              // trusted_keys + mode, merged: workspace, tightened by local
    pub max_artifact_size: u64 }
```

Why: these are the user-visible controls A, C and D.

## 5. Test plan

- **Unit:** scope-to-target authorization matrix, namespace derivation and scope
  validation, trailer encode/decode and bounds, signed-message canonicalization,
  sign and verify round trip, every failure above, mode matrix, key rotation.
- **Storage contract (`omni_remote_cache_storage_core/test_utils`):** atomic
  create-if-absent under concurrency, commit-only-on-complete (an upstream `Err`
  commits nothing), range, `list_prefix` and `delete_prefix`. Local and memory run it
  through the existing `decl_remote_cache_storage_backend_tests!` macro. S3 is not in
  that suite today, so this RFC adds it against MinIO or LocalStack (the current S3
  test setup is checked first). R2 targets wasm32 and cannot run under `cargo test`,
  so it is covered by the Miniflare script plus a manual run on real R2.
- **Trust boundaries (4.1, 4.4):** a publisher and a read-only run never send an
  isolated probe, even when the server would answer it; an untrusted run does; a
  local config cannot lower `verify` or add `trusted_keys`; `scopes: all` cannot
  `PUT` to `shared`; the S3 backend rejects the removed bucket-per-container mode.
  The Goal 4 failure guard (3): a test fails if a publisher or read-only run restores
  anything from an isolated namespace.
- **Marker rules (4.4):** a run with `publish` treats an `isolated`-marked local entry
  as a miss and replaces it with a local success; an untrusted run (`write`, no
  `publish`) reuses it; a `shared`-marked entry is reused by every run; an opted-out
  run misses on every marked entry; unknown scopes (remote disabled) are treated as
  trusted. `CachedSecurity` never serves a `Caller` built for another resource or
  wanted set. `auto` is resolved against the workspace key set before narrowing, and
  an empty local subset under `require` rejects every shared entry.
- **Path safety:** `..`, leading-dot and encoded segments in `scope` and
  `principal_id`, malformed digests, a resolved path outside the container root, and
  a crafted cached `original_path` (absolute, `..`, outside the project root), each
  rejected.
- **Seam test:** a test-only authenticator returns a `Caller` with a non-API-key
  credential kind, a path-unsafe principal (`repo:org/name:ref:refs/heads/x`), and a
  `scope_pin`. The worked example runs against it unchanged: the principal is
  encoded to a safe path segment, the client's `scope` is ignored in favor of the pin,
  and provenance and hooks record the credential kind. This fails if anything
  downstream of the authenticator assumes API keys.
- **Integration (`omni_remote_cache_core`):** the worked example against the
  in-memory data and storage implementations, including hooks, provenance and
  purge.
- **Client (`omni_cache`):** `push: false` makes zero `PUT` calls; `409` is
  success; an upload failure (`403`, `413`, `5xx`) is a warning and does not fail the
  run; failed executions are not uploaded; `cache.remote: false` skips
  remote reads and writes; verification runs before `unarchive`.
- **End to end (`tests/omni-tests`):** the CREEP scenario (poisoned PR artifact is
  not restored by a `main` build), and the server-compromise scenario (tampered
  bytes are a miss).
- **Failures guarded against:** (1) an untrusted writer's artifact restored by a
  trusted build: a test must fail if the shared lookup consults other keys'
  namespaces. (2) a tampered shared artifact extracted: a test must fail if
  verification is skipped before `unarchive`.

## 6. Rollout

No compatibility layer. Land as thin slices, each releasable. Each slice only uses
things earlier slices provide:

1. Storage: create-if-absent, `get_range`, `list_prefix`, `delete_prefix`, path
   safety (digest and containment checks), server `409`, truncated-upload abort,
   remove `replacing`, and in the same slice the client uploads only successful
   executions (4.2), treats `409` as success and upload failures as warnings (B).
   The success-only gate must ship with immutability: otherwise a failed entry
   could occupy a digest forever.
2. Authenticated `Caller` seam (4.0): `Credential` extractor, per-request
   `Authenticator::authenticate(credential, resource, wanted)`,
   `CachedSecurity`, `validate_access`, SQL/D1 `principal` column, `X-API-KEY` to
   `Authorization`.
3. Trailer, sealing stream, server-authored provenance, hooks contract and SQL
   hooks parameters (F, part 1).
4. Scopes, namespaces and client-driven resolution (C).
5. Client config and defaults: `push`, `target`, `scope: auto`, size caps, and the
   split between the committed workspace config (`trusted_keys`, `verify`) and the
   local file, with the never-loosen merge (A).
6. Per-task `cache.remote: false`, `--no-remote-cache`, marker with staged
   extraction, metadata path validation (E).
7. Signing: `keygen`, sign on shared upload, verify before `unarchive`, modes (D).
8. Operator tools: `cache provenance`, `cache purge`, isolated TTL (F, part 2).
9. Docs: remote cache guide with a CI recipe (GitHub Actions environment-scoped
   secrets for `publish` and the signing key), security page, release note listing
   the breaking changes.
10. End-to-end CREEP and server-compromise tests, run before the RFC is Implemented.

Undo: each slice is a commit on `main` and can be reverted. Data is not migrated.
Existing remote entries use the old container layout and have no trailer, so
they become misses and operators clear them with `omni cache purge --legacy`. The release note says so. At
runtime, `verify: off` disables verification and `push: false` stops uploads.

## 7. Alternatives and review questions

**Rejected**

- **Shared-secret HMAC signing (Turborepo).** Every verifier can forge, including
  the developers this RFC does not trust.
- **Read-only vs read-write only (Gradle/Bazel).** Safe, but untrusted PR builds
  get no cache of their own, which pushes teams to loosen the rules. Isolation keeps
  the speed.
- **Per-environment scopes instead of namespaces.** Scopes are per key today and
  environments are caller-chosen, so isolation would depend on operator
  discipline. Server-derived namespaces cannot be misconfigured that way.
- **Server-side read-through (isolated then shared).** Makes the client trust a
  server header about where a hit came from. Client-driven probing removes that
  trust, at the cost of one extra request for keys that can write.
- **Sidecar signature or metadata object.** Not atomic with the artifact, so a
  crash leaves an immutable, permanently unverifiable entry. The single-object
  entry with a trailer avoids that. A leading header would force the server to
  buffer or rewrite the body, since the hash is known only after streaming.
- **Multipart uploads on the commit path.** The R2 Workers binding cannot condition
  `complete`. The size cap (4.2) avoids it.
- **Content-addressed artifacts.** Prove integrity, not origin, and conflict with
  input-hash keys.
- **Overwrite with a `replace` scope.** Needed only to swap failed entries, which
  4.2 removes.
- **Sigstore keyless signing.** Strong CI identity, but a heavy dependency and a
  network trust path. Possible later replacement for static keys.

**Decisions that closed earlier open questions**

- **CI variables for `scope: auto`:** table in 4.4. Default is per PR or branch,
  not per run, to keep sharing across reruns. `scope: run` is available.
- **Conditional writes:** S3 supports `If-None-Match: *` on `PutObject` and
  `CompleteMultipartUpload`. R2's S3 API does too, atomically. The Workers binding
  exposes `only_if` on `put` but not on multipart `complete`, so entries are
  size-capped and committed with one conditional write (4.2).
- **Streaming and hashing:** spool while hashing, append the trailer, commit
  conditionally (4.3). On Workers the entry is buffered, so the cap is smaller.
- **`max_artifact_size`:** 256 MiB native, 32 MiB on Workers (128 MB isolate
  memory; incoming request bodies are also capped by the zone plan at 100 MB on
  Free and Pro and 200 MB on Business). Both are configurable. The client reports
  the size of any artifact it skips, so operators can tune the cap from real data.
- **TTL:** 7 days by age, bucket lifecycle rules for object stores, a sweeper for
  local and memory, server-wide (4.6).
- **`cache.remote: false`:** applies to transitive dependencies. Remote-origin
  local entries are marked by a separate marker file, so no local cache format
  change or reset is needed (4.4).
- **`list:artifacts`:** namespace-aware (4.6).
- **Purge cost:** isolated-by-principal is a prefix delete. Other purges scan with
  suffix range reads. No index until measured (4.3).
- **Principal from an identity provider:** optional `principal` column in the key
  query contract, filled by the issuer (4.1).
- **Authenticator chain:** first match wins, in config order, and credential
  formats must be unambiguous (4.0).
- **Isolated reads and verification:** only untrusted runs (`target: isolated`, no
  `publish`) probe isolated. Trusted and read-only runs never do (4.1). The review
  recommended also disabling isolated reads in `require` mode outright. That would,
  with workspace-level `trusted_keys`, switch off isolated caches for PR CI in every
  signing-enabled workspace and defeat goal 3, so the rule is on who reads, not on the
  mode.
- **Where `trusted_keys` and `verify` live:** the committed workspace config, merged
  with the local file so the local file can only tighten (4.4).
- **Layout and lifecycle:** namespace first, `shared/...` and `isolated/...`, one
  lifecycle prefix, and S3 key-prefix mode only (4.1).
- **`Caller` and the SQL/D1 contract:** authenticate per request with the resource
  and wanted scopes. `scopes: all` never grants `publish` (4.0).

**Open questions (owner TBD)**

1. Does the R2 **Workers binding** (`put` with `onlyIf`) give create-only on **real**
   R2? Spike results (2026-11-13):
   - **Real R2, S3 API: confirmed.** `PutObject` with `If-None-Match: *` stored on an
     absent key and returned `412 PreconditionFailed` on an existing one, leaving
     the value unchanged. Five rounds of 10 concurrent `PutObject`s to one key each
     produced exactly one winner. `CompleteMultipartUpload` with `If-None-Match: *`
     behaved the same (stored when absent, `412` when present, original bytes kept).
   - **Miniflare (simulator), Workers binding: confirmed** (Miniflare 4.20260730.0;
     both `etagDoesNotMatch: "*"` and a `Headers` object; 10 concurrent: one winner).
     This contradicts the 2024 report (workers-sdk #6411), so that simulator bug is
     fixed.
   - **Real R2, Workers binding: not tested.** Running a Worker (including
     `wrangler dev --remote`) needs a `workers.dev` subdomain on the account, which is a
     persistent account change that the spike did not make.

   Consequence for the design: create-only is proven on real R2 through the S3 API, so
   the R2 backend has a working path either way. If the binding also passes on real R2
   it can stay the Workers implementation, otherwise the R2 backend uses the S3 API
   (SigV4 `PutObject` with `If-None-Match: *`). Plan step 1 verifies the binding when a
   Worker can be deployed.

**Acceptance checklist (not a decision)**

Before status moves to Accepted: re-verify the tool comparison in section 2 and the
CI variable table in 4.4 against current docs, since both change.

## 8. Implementation status and deviations

Not started. Update this section as work proceeds.

### Plan (steps, checks, undo)

Riskiest parts first: step 1 (atomic create-if-absent and path safety across
backends, open question 1) and, within step 3, the sealing stream.

| # | Step | Check | Undo |
| --- | --- | --- | --- |
| 1 | Storage primitives, path safety, `409`, abort on truncated upload, remove `replacing`, client success-only upload gate and `409`/warning handling. | Concurrency and abort contract tests per backend, path-safety tests, a failed execution is never uploaded. Spike on real R2 and S3: create-only conditional write. | Revert commit. |
| 2 | `Caller`/`Authenticator` seam and header move. | Seam test, existing end-to-end remote tests still pass. | Revert commit. |
| 3 | Trailer, sealing stream, provenance, hooks and SQL hooks params. | Trailer bounds tests, mismatch leaves the key free, provenance not client-settable, suffix-range read returns the trailer, `409` logged as `AlreadyExists`. | Revert commit. |
| 4 | Scopes, namespaces, client-driven resolution. | Authorization matrix, worked-example integration test, failure guard (1). | Revert commit. |
| 5 | Client `push`/`target`/`scope`, size caps. | Zero-upload test, advertised-cap test. | Revert commit. |
| 6 | `cache.remote: false` (transitive), CLI flag, marker with staged extraction, metadata path validation. | Remote reads and writes skipped for the task and its dependencies, marked entry is a miss then replaced by a local success, a crash mid-extraction leaves no unmarked remote entry, hostile `original_path` rejected. | Revert commit. |
| 7 | Signing: `keygen`, sign, verify before `unarchive`, modes. | Full failure list in 4.7, failure guard (2). | Revert commit. |
| 8 | `cache provenance`, `cache purge`, isolated TTL. | Purge removes only the named writer, key or signing key; TTL touches isolated only. | Revert commit. |
| 9 | Docs and release note. | Docs build. A reviewer follows the CI recipe against a test server. | Revert docs. |
| 10 | End-to-end CREEP and server-compromise tests. | Both fail if the matching guard is removed. | n/a |

Because this is High risk, a second reviewer reads the RFC before step 1 and the
diff for steps 1, 2, 3, 4, 7 and 10.

What would make this larger: needing VCS integration to verify scope labels,
keyless (Sigstore) signing, or an artifact index in a database. Reassess if any
appears.
