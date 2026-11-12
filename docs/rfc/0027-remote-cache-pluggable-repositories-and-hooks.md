# RFC 0027: Pluggable repository backends and entry hooks for `omni_remote_cache_core`

- **Status:** Draft
- **Owner:** TBD
- **Date:** 2026-11-12
- **Base branch:** `main`
- **Risk:** High (auth and tenancy checks, and a public extension contract)
- **Kind:** Large
- **Affected crates:** `omni_remote_cache_core`, `omni_remote_cache_service`, `omni_remote_cache_worker`, new backend crates (names in section 5)

## 1. Goal and done

Today `omni_remote_cache_core` reads tenants, organizations, workspaces and environments from an in-memory store built from config (`ConfigBasedDependencyProvider`). This RFC makes that data come from interchangeable backends and adds hooks around artifact reads and writes.

Done means all of these hold:

1. **SQL backend.** Reads ownership data from any database `sqlx` supports on native, using operator-supplied queries.
2. **D1 backend.** Reads the same data from Cloudflare D1 inside `omni_remote_cache_worker`, using operator-supplied queries.
3. **Query shape contract.** Any schema works as long as the query returns the documented shape. Startup validation checks the columns and parameter count where the driver can describe the query. Row values and row counts are checked on each request. A NULL or unsupported `found` value, or more than one row, is a shape error detected at request time. A request-time shape error is logged and metered, and the request fails with a 500 (fail closed, never treated as "found").
4. **Embedder-supplied data.** A host can supply its own data without SQL by implementing the repository traits (or a closure-based adapter). This covers ownership data and API keys and scopes, so any backend can supply them.
5. **Hooks.** Hooks run on every operation the service has (get, get-many, head, head-many, put, delete), and can **veto** the operation.
6. **SQL hooks backend.** A SQL (and D1) hook backend lets an external service prune old or unused entries.
7. **Caching decorator.** There is no caching by default. A caching decorator that wraps any repository (and the security service) is available to opt into.
8. **No regression.** The in-memory backend and today's config behave exactly as before.
9. **Executable example.** The worked example in section 3 runs as a test.

**Out of scope:**

- A REST-based repository backend (backlog). The trait design must not preclude it.
- Cloudflare KV and R2 as repository backends. D1 only. R2 stays a storage backend.
- A pruning service itself. Hooks only record data for one.
- Changes to the artifact storage backends.

## 2. What it touches

| Area               | Where                                                                                                                        | Effect                                                                                                                                     |
| ------------------ | ---------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| Repository traits  | `data/{tenants,organizations,workspaces,environments}/repository.rs`                                                         | The traits stay the extension point; their shape is reviewed in step 1.                                                                    |
| Providers          | `providers/{traits,config}.rs`, `DependencyProvider`                                                                         | Adds backend selection and a hooks accessor.                                                                                               |
| Validation         | `services/validation*.rs`                                                                                                    | Consumes the repositories; may collapse several round trips into one query.                                                                |
| Security           | `security/` (`SecurityService`)                                                                                              | API keys and scopes become pluggable, with the same backends and query contract as the repositories.                                       |
| Routes             | `routes/v1/artifacts/{get_artifact,get_artifacts,head_artifact,head_artifacts,put_artifact,delete_artifact}.rs`, `common.rs` | Hook call sites and veto responses in all six handlers.                                                                                    |
| Config             | `config/mod.rs`                                                                                                              | New backend and query configuration.                                                                                                       |
| Consumers          | `services/omni_remote_cache_service`, `services/omni_remote_cache_worker`                                                    | Wire the chosen backend. The worker is wasm.                                                                                               |
| Runtime constraint | wasm32 + D1                                                                                                                  | D1 futures are not `Send`. Decided: keep the traits `Send` and wrap JS futures in the D1 backend (section 5; RFC 0019, "`Send` strategy"). |

## 3. Worked example

An operator runs the Worker with D1 and an external pruning service.

**Happy path**

1. Config points ownership lookups at D1 with these queries:
    - `SELECT 1 AS found FROM orgs o JOIN tenants t ON t.id = o.tenant_id WHERE t.code = ?1 AND o.code = ?2`
    - Equivalents for the workspace and environment checks.
2. `PUT /v1/artifacts/abc?org=o&ws=w&env=e` passes auth and ownership validation. The `before` hook allows it, the artifact is stored, and the `after` hook inserts a row in `artifact_events`.
3. `GET` of the same artifact runs the `before` hook, which allows it, then serves it and upserts `last_read_at` in `artifact_usage`.
4. The external pruner reads `artifact_usage`, deletes cold entries, and the next `GET` returns 404. Usage can be stale (a failed `after`, sampled or batched writes), so a recently read entry can look cold. The pruner contract: it must only delete entries idle for longer than a minimum age that exceeds the expected usage-update delay (sampling or batch interval plus `after` timeout), or accept that a wrongly pruned entry is a cache miss and a rebuild, not data loss.

**Complexities**

- The operator's schema differs from ours. Column names differ, there is a soft-delete flag, and there are extra joins. The query adapts the schema to the contract by aliasing columns.
- The same contract must work on SQLite, Postgres and D1 (positional versus named parameters).
- A veto must stop the operation before bytes are stored or served, including on streaming uploads. A `before` veto on put must not leave a partially stored object.
- Hooks must not make reads slow or unavailable by accident. Recording `last_read_at` must not turn every read into a blocking write.

**Edge cases**

- The query returns no rows, extra rows, an unexpected column or the wrong type. Each is a defined error.
- A hook errors or times out. Behaviour is per-hook policy: `fail_open` or `fail_closed`, default `fail_closed` for `before` hooks.
- A hook vetoes with a reason. The client gets 403 with the reason, and the veto is logged.
- The `after` hook fails. The operation has already succeeded, so it is logged and metered, not surfaced to the client.
- The database is unreachable. Ownership validation returns 500 and never allows by default.
- A SQL injection attempt via org/ws/env values. Values are bound parameters only, never interpolated.

## 4. Proposed design (draft; settled by step 1)

### Repository data

Keep the existing repository traits as the public extension point. Embedders implement them directly or use closure adapters. The SQL and D1 backends implement them from configured queries.

### Query shape contract

Each query is declared in config with a name, the SQL text, and an ordered list of bound parameters:

```yaml
repository:
    kind: sql # sql | d1 | in_memory
    queries:
        tenant_exists:
            {
                sql: "SELECT 1 AS found FROM tenants WHERE code = $1",
                params: [tenant_code],
            }
```

- **Row shape.** Every query returns zero or one row with the documented columns. `exists` queries return a column named `found` that is a boolean or an integer. Non-zero means true. Any other type, including NULL, is a shape error. Zero rows means false.
- **Parameter binding.** The SQL text is passed to the driver verbatim. We do not rewrite placeholders, and the operator writes them in the dialect of their driver (`$1` for Postgres, `?` for MySQL and SQLite, `?1` for D1). The configured `params` list is bound in order, so the n-th entry binds to the n-th placeholder. Values are always bound, never interpolated.

### Hooks

```text
trait ArtifactHooks {
  before(&ctx) -> HookDecision          // Allow | Veto { reason }
  after(&ctx, &outcome)                 // observe, cannot veto
}
ctx = { tenant, org, ws, env, op, digests, api_key_id, size? }   // digests: one for single ops, all requested for *Many
op  = Get | GetMany | Head | HeadMany | Put | Delete
```

- **Timeouts.** Each hook has a configurable timeout. Defaults: `before` 2 seconds, `after` 10 seconds. A timeout counts as a hook error and follows the hook's failure policy (default closed for `before`). Each hook can override its own timeout and policy in config.
- **Ordering.** Hooks run after auth and ownership validation, before storage access. Multiple hooks run in registration order, and the first veto wins.
- **Async and deadlines.** Hooks are async with a per-hook timeout.
- **`after` on the request path.** `after` hooks run after the response is decided. On native they are spawned. On Workers they use `waitUntil`.
- **SQL hooks backend.** It uses configured queries to record events or usage, and optional queries to veto. A `before` query that returns a `veto_reason` column vetoes with that reason.
- **Batch operations.** `GetMany` and `HeadMany` call `before` once with the full `digests` list. A veto rejects the whole batch (403 with the reason), because partial results would hide the veto from the client. `after` receives the per-digest outcome (hit, miss) so usage is recorded for each artifact.
- **Usage writes.** To avoid a write on every read, the SQL hook can sample or batch `last_read_at` updates (configurable granularity).

### API key hashing

The service transforms the presented key before it touches the database, so the operator can store keys hashed or plain.

```yaml
security:
    api_key_hash:
        algorithm: sha256 # none | sha256 | sha512 | hmac-sha256
        encoding: hex # hex | base64
        secret: ${KEY_PEPPER} # only for hmac-sha256
```

- **Flow.** The service computes `stored = encode(hash(presented))`. It binds `stored` as the query parameter, and the query returns the `api_key` column plus scopes. The service compares the returned value with `stored` in constant time before trusting the row.
- **Defaults.** `sha256` with `hex` for the SQL and D1 backends, so keys are not stored in the clear unless the operator chooses `none`. The existing config-file backend keeps `none`, so it behaves as it does today.
- **Only deterministic hashes.** The lookup is by key value, so there is no per-key salt or id to look up first. Slow salted hashes (argon2, bcrypt) are therefore not supported. API keys are expected to be long random values, where a fast hash is appropriate. `hmac-sha256` adds a server-side secret, so a leaked table alone cannot be checked offline.
- **Portable.** The hash crates are pure Rust (`sha2`, `hmac`), so they build for wasm32 and run in the worker.
- **No secrets in logs.** Neither the presented key, the computed hash nor the secret is logged. The existing redaction applies.
- **Changing the algorithm** invalidates stored values, so the operator must migrate their rows. This is the operator's schema (open question 5), and the docs say so.

### Caching decorator

Backends never cache. A separate decorator wraps any implementation of the repository traits (and `SecurityService`), so it works over SQL, D1, in-memory or an embedder's own backend.

- **Opt-in.** Off by default. Enabled per repository in config with a TTL and a maximum entry count.
- **Key and value.** The key is the method plus its arguments, and the value is the result. Errors are never cached.
- **Negative results.** Cached only if configured, with a separate (shorter) TTL, so that a newly created tenant is not rejected for long.
- **Scope.** Per process, and per isolate on Workers, where it lives only as long as the isolate. It is runtime-agnostic: no Tokio, with a clock that works on wasm (`time` with `wasm-bindgen`).
- **Defaults when enabled.** Repositories: TTL 60 seconds, 10,000 entries, negative results not cached. Security service: TTL 10 seconds. All configurable.
- **Security caveat.** Caching API key and scope lookups delays revocation by up to the TTL, and caching ownership lookups in the repositories delays ownership changes (a deleted tenant, organization, workspace or environment staying valid) by up to the repository TTL (60 seconds by default). Both bounds are documented side by side. The docs must say so, and the default TTL for the security service is shorter than for the repositories.
- **Concurrency.** A concurrent miss for one key may query twice. No request coalescing in the first version.

### Wiring

`DependencyProvider` gains `artifact_hooks()`. A new `BackendConfig`-driven provider replaces the hard-wired in-memory repositories. `ConfigBasedDependencyProvider` stays as the default.

## 5. Options considered

| Decision            | Options                                                                                              | Lean                                                                                                                                                                                                                              |
| ------------------- | ---------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| SQL layer           | A: `sqlx` 0.9.0 for native. B: per-driver crates behind our own trait.                               | **Decided: `sqlx` 0.9.0 on native**, covering whatever it supports (SQLite, Postgres, MySQL). D1 is a separate implementation, because `sqlx` does not run on wasm32. Both sit behind the same traits and query contract.         |
| Backend crates      | One `omni_remote_cache_repository_sql` crate plus one `_d1` crate, or one crate with features.       | Separate crates, mirroring `omni_remote_cache_storage_*` from RFC 0019.                                                                                                                                                           |
| Query contract      | Free-form SQL with a fixed shape, a mini query DSL, or fixed schema with a column mapping.           | Free-form SQL with a fixed shape. It meets "no matter the schema".                                                                                                                                                                |
| Hook failure policy | Always fail-closed, always fail-open, or per-hook.                                                   | Per-hook, default closed for `before`.                                                                                                                                                                                            |
| `Send` on wasm      | A: keep traits `Send`, wrap JS futures in the D1 backend. B: `?Send` on wasm. C: `?Send` everywhere. | **Decided: A.** Traits and core stay `Send`. The D1 backend wraps futures in `worker::send::SendFuture` and holds handles in `SendWrapper`, as `omni_remote_cache_storage_r2` already does. B is the fallback if the spike fails. |
| Caching             | A: built into each backend. B: one decorator over the traits.                                        | **Decided: B**, opt-in and off by default. Backends stay simple and an embedder's backend gets caching too.                                                                                                                       |
| Hook trait          | Core trait with SQL as one implementation, or SQL-only hooks.                                        | Core trait, which keeps the REST backlog item open.                                                                                                                                                                               |

## 6. Plan

### Step 0: Spike (riskiest unknowns first)

- Prove that one `exists`-style query contract runs through `sqlx` (SQLite, Postgres, MySQL) and D1 (wasm).
- Confirm the `Send` strategy (option A) holds for the repository, security and hook paths on wasm. Fall back to option B only if it fails.
- Confirm that a `waitUntil`-style post-response hook can be expressed from `omni_remote_cache_core`.

**Check:** a throwaway test or branch that runs the same query through both a native driver and a Worker build (`wrangler dev` or miniflare). **Undo:** discard the spike.

**Spike result (done, code in `/tmp/opencode/spike`, not committed).** Run against real engines: `sqlx` 0.9.0 `Any` with SQLite, Postgres 16 and MySQL 8, and D1 under `wrangler dev --local`. The wasm build also compiled for `wasm32-unknown-unknown`.

| Question                      | Result                                                                                                                                                                                                                                                       |
| ----------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Query contract through `sqlx` | Works. Verbatim SQL, positional string binds, `found` column, zero rows means false. Verified for `?`, `?1` (SQLite), `$1` (Postgres) and `?` (MySQL). Injection strings are inert.                                                                          |
| Same contract on D1           | Works with `?1`. Same results as SQLite.                                                                                                                                                                                                                     |
| `Send` option A               | Works. `Send` traits, with `SendWrapper<D1Database>` and `SendFuture` in the D1 backend, compile and run on Workers. Core needs no change.                                                                                                                   |
| `after` via `waitUntil`       | Works. A `HostRuntime` trait (`spawn_detached(BoxFuture + Send)`) in core, implemented by `tokio::spawn` natively and `ctx.wait_until` on Workers (context held in `SendWrapper`). The event was written after the response (0 to 1), and a veto skipped it. |
| Veto                          | Works end to end. A `before` query returning `veto_reason` gave 403 with the reason and no `after` write.                                                                                                                                                    |

Findings that change the design:

1. **`found` typing is not portable.** Postgres returns `BOOLEAN` for `(x = 0) AS found`, and decoding it as an integer fails. The contract must accept bool or integer, or require operators to cast. **Decided: the contract accepts a boolean or an integer** (non-zero means true, zero means false) and returns a shape error for any other type, including NULL. Tests cover both types on each driver we ship.
2. **A mismatched dialect is not always an error.** SQLite and D1 accepted `$1` and bound it positionally. Postgres and MySQL reject the wrong style. Docs must not promise a uniform error, and startup validation must not rely on one.
3. **`D1Database` is not `Clone`.** Each backend calls `env.d1(binding)` itself. The worker provider needs a binding name, not a shared handle.
4. **`after` needs a core abstraction.** `HostRuntime` is a new trait in core and is part of the contract in step 1, because tasks are `'static` and cannot borrow the request.
5. **`sqlx` 0.9 rejects dynamic SQL text.** `sqlx::query` takes a `SqlSafeStr`, so operator-configured query strings must be wrapped in `sqlx::AssertSqlSafe`. That is correct for us because the text comes from operator config, never from request data. Wrap in one place only (the backend's query function), document why next to it, and never pass request values into the SQL text. A test must cover that request values only ever reach the query as bound parameters.
6. Only string parameters were tried. Typed parameters are not required by the example, and stay out of scope unless step 1 finds a need.

Not covered: the full `ArtifactHooks` shape, timeouts on hooks, and API-key queries.

### Step 1: Contracts (RFC amendment and traits, no behaviour change)

Fix the repository trait shape, the query contract, the `ArtifactHooks` trait and the config schema. Add the hooks accessor with a no-op implementation. The in-memory backend continues to work.

**Check:** the existing tests pass, and clippy, fmt and `cargo test -p omni_remote_cache_core` are clean. The service and worker still compile. **Undo:** revert the commit.

### Step 2a: Caching decorator

**Check:** unit tests cover TTL expiry, the maximum size, no caching of errors, the negative-result TTL, and that a disabled decorator calls through. They also cover revocation delay being bounded by the TTL. The crate builds for wasm32. **Undo:** config-selected; absent means no caching.

### Step 2: SQL repository backend (native)

**Check:** integration tests against each `sqlx` driver we ship (SQLite in-process, Postgres and MySQL in CI containers). The edge cases in section 3 are covered, including a wrong-shape query and an injection attempt. For API keys: each hash algorithm and encoding, a wrong key, a stored value that differs from the computed hash, and a check that no key, hash or secret appears in logs. **Undo:** the backend is config-selected; the default is unchanged.

### Step 3: D1 repository backend (worker)

**Check:** the wasm build, plus a worker test against local D1. **Undo:** as step 2.

### Step 4: Hooks in routes, with veto

Add call sites in all six handlers (get, get-many, head, head-many, put, delete).

**Check:** tests per operation for veto (403, with nothing stored, served or deleted), hook error under both policies, and `after` failures. **Undo:** with no hooks configured the behaviour is identical to today.

### Step 5: SQL and D1 hooks backend

Event and usage recording, plus optional veto queries. All are operator-supplied queries against the operator's own schema (no migrations from us).

**Check:** the section 3 example end to end on SQLite and D1.

### Step 6: Integration

Run the full set once (see below), and write the user docs in `apps/site` and the release note. Remove the spike code.

Steps 2a, 2, 3 and 5 can run in parallel after step 1. Step 4 depends on step 1 only. Integration stays with one owner.

## 7. Checks and risk

Failures being guarded against:

- A wrong ownership or scope result that grants cross-tenant access, or accepts a bad API key. Test the SQL contract with negative cases.
- Injection through org, ws or env values. These are bound parameters only.
- Hooks that fail open silently. The default is closed for `before`, with logging.
- Hook latency or write amplification on the read path.

Checks:

- Per crate: `cargo clippy -p <crate> -- -D warnings`, `cargo fmt -p <crate> -- --check`, `cargo test -p <crate>`.
- At integration: the wasm build of `omni_remote_cache_worker`, plus the whole set once.
- A reviewer other than the author reads steps 1 and 4 (auth boundary).

## 8. Assumptions and open questions

| #   | Item                                                                                                                                                                                                                                                                                                                                                                                                                                                                | Owner   | Blocks |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------- | ------ |
| 1   | ~~Hook scope~~ Resolved: all operations the service has.                                                                                                                                                                                                                                                                                                                                                                                                            | Product | -      |
| 2   | ~~Pluggable API keys~~ Resolved: yes, via the same backends, with **configurable key hashing** (section 4, "API key hashing").                                                                                                                                                                                                                                                                                                                                      | -       | -      |
| 3   | ~~Databases~~ Resolved: whatever `sqlx` supports. Each `sqlx` driver is its own cargo feature (for example `sqlite`, `postgres`, `mysql`), **on by default**, and a consumer can turn them off with `default-features = false` and pick its own. The `sqlx` dependency is gated to non-wasm targets, so the default features never reach the worker build, which uses D1 only. Config naming a driver that was not compiled in fails at startup with a clear error. | -       | -      |
| 4   | ~~Repository caching~~ Resolved: not by default, with an opt-in caching decorator (section 4). Default TTL values and the config shape are settled in step 1.                                                                                                                                                                                                                                                                                                       | Design  | Step 1 |
| 5   | ~~Schema ownership~~ Resolved: the operator owns all tables and migrations for ownership data, API keys, and hook events and usage. We ship no migrations and no DDL that runs automatically. The operator supplies queries that return the shape the service expects, and the docs list each query, its parameters and its columns. Example DDL appears in the docs for illustration only.                                                                         | -       | -      |
| 6   | ~~Placeholder dialects~~ Resolved: no rewriting. The operator writes queries in their driver's dialect. Docs must show an example per driver.                                                                                                                                                                                                                                                                                                                       | -       | -      |

## Implementation notes

Where the build differs from the plan above:

- **API key check.** Every key check is an `exists` query on the hashed key, so the database compares the hash and the service does not compare a returned value itself. The lookup is by a hash the caller cannot choose, so a timing attack on the comparison has nothing to exploit.
- **Batch hooks.** `GetMany` and `HeadMany` are the list operations. They have no digests, so `before` runs once with an empty `digests` and a veto rejects the request. Single-artifact operations carry one digest.
- **Crates.** `omni_remote_cache_repository_query` (contracts, repositories, security, hashing, hooks, config), `_sql` (`sqlx` 0.9.0, native) and `_d1` (Workers). The caching decorator is `omni_remote_cache_core::caching`.
- **Timers.** Core has no runtime, so `HostRuntime` also provides `sleep`, used for hook timeouts. Without a `HostRuntime`, `after` runs inline and timeouts are not enforced.
- **D1 strictness.** D1 rejects a query that binds more parameters than it references. Operators must reference every parameter. Verified under `wrangler dev --local`.
- **Verified.** SQLite, Postgres 16 and MySQL 8 for the executor, and the whole worker under `wrangler dev --local` (auth, validation, usage recorded with `waitUntil`, veto). Postgres and MySQL tests run when `TEST_POSTGRES_URL` or `TEST_MYSQL_URL` is set.
- **Startup validation.** The first version checks that every query is present and non-empty. It does not prepare queries against the driver, so invalid SQL or a missing column surfaces on first use (a 500, logged). Driver-backed preparation at startup is follow-up work.
- **Statement deadline.** Native statements are bounded by `query_timeout_ms` (default 10000).
- **TLS.** The SQL crate enables `sqlx/tls-rustls` by default (`tls` feature). The service refuses a remote Postgres or MySQL url that does not set `verify-full` / `VERIFY_IDENTITY` (`verify-ca` does not check the host name), unless `allow_insecure_tls` is set. It checks what sqlx will apply: the last TLS-mode parameter wins, and `host` and `hostaddr` replace the url's host. Loopback and unix-socket hosts are exempt.
- **Caching.** Expiry uses a monotonic clock natively and the wall clock on wasm32. On Workers the cache state is held per isolate so entries survive between requests. `CachingProvider` delegates ownership validation to the wrapped provider and caches only the result, so the wrapped provider's own rules still apply.
- **Startup validation.** The native executor prepares every configured query at startup. It checks the parameter count (SQLite allows fewer, because it reports the highest index used) and the `found` or `veto_reason` column, and a wrong query stops the service. D1 cannot describe a query, so it keeps first-use failures. Row values and row counts are still checked per request.
- **`api_key_id` in the hook context.** An opaque fingerprint (16 hex characters of the key's SHA-256). It is the last parameter of the `before` query and follows the outcome in the `after` query.
- **Hook isolation.** `after` hooks run on their own pool (`hooks_max_connections`, default 2) and `after_max_in_flight` defaults to that size, so slow usage writes cannot starve authorization lookups or `before` hooks, which stay on the main pool because they are on the request path. With in-memory SQLite or `hooks_max_connections: 0` the pool is shared and a slow `after` hook can delay authorization.
- **Tenant isolation of storage.** Artifacts are stored under `tenant/org/ws/env`. This is a change from `org/ws/env`: existing artifacts are unreachable after upgrading and are rebuilt as cache misses.
