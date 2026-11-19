# RFC 0030: client configuration rollout

Plan for slice 5 of [RFC 0030](0030-remote-cache-trust-model.md) (section 4.4, the parts that do
not need signing or local partitions, and 4.8.6/4.8.9). The RFC stays the design authority.

## Assessment

Medium-to-large, High risk by effect: it changes when the client uploads (the default becomes
never), what it asks the server for, and how much it will download. One owner; an independent Opus
review of this plan and of the build before commit.

## Goal and done

- Machine-local remote cache config gains `push` (default `false`), `target` (`isolated` default,
  or `shared`), `scope` (`auto` default, `run`, or a literal label) and `max_artifact_size`
  (optional, bytes). Old files (YAML and encrypted msgpack, which stores structs as arrays) still
  load with these defaults.
- `omni cache remote setup` takes `--push`, `--target`, `--scope`, `--max-artifact-size` and writes
  them (the encrypted file cannot be hand-edited).
- Uploads happen only when `push` is true. `target: shared` needs `publish:artifacts` and an
  isolated upload needs `write` or `publish`, judged from the probe's scopes; with scopes known and
  insufficient, the upload is skipped with a warning, never an error. Any upload failure is a
  warning (already true).
- The client sends the effective scope label (`scope=`) on isolated requests. The label is
  `auto`: `pr-<n>` for a pull request, else the branch, else `default`, for the CI providers in
  section 4.4 (GitHub Actions, GitLab, CircleCI, Buildkite, Jenkins); `run` uses the provider's run
  id; a literal is used as given. The result is made safe for `[A-Za-z0-9][A-Za-z0-9._-]{0,63}`:
  other characters become `-`, a leading non-alphanumeric gets an `x` prefix, and a label that is
  too long or changed is shortened to a prefix plus `-` and 8 hex of its SHA-256 (so two long
  branches stay distinct). An empty result is `default`. A literal that is invalid is a
  configuration error at setup, a warning and `default` at run time. A pin on the credential still
  wins on the server; the probe's `X-Omni-Scope` is what the client records as effective.
- Size caps: effective cap = the smaller of the local `max_artifact_size` and the server's probe
  value, with a built-in default of 256 MiB when neither is known. Uploads over the cap are skipped
  with a warning; downloads are read as a bounded stream (declared `Content-Length` over the cap, or
  a body that exceeds it, is a miss), so an unbounded `bytes()` is gone.
- One probe per store per run (lazy, shared by all tasks) yields scopes, principal id, scope label
  and size cap. A failed probe leaves them unknown: scopes unknown means upload is attempted if
  `push` is on (the server decides), and the default cap applies.

Out of scope: signing and `verify` (slice 7), local partitions, trust epoch and origin markers
(slice 6), `cache.remote: false` and `--no-remote-cache` (slice 6), operator tools (slice 8).

## Decisions

1. **Where config lives.** All of this is machine-local and in `RemoteCacheConfiguration`
   (`serde(default)` on each new field), as the RFC says; workspace-committed policy is signing
   only and comes in slice 7.
2. **Breaking default.** `push` defaulting to `false` stops uploads for existing setups until they
   re-run setup with `--push` or set `push: true`. The RFC requires it; the release note and
   guide say so. Reads are unaffected.
3. **Placement.** Reads stay isolated then shared (the shared fallback is also needed for a
   read-only key). `target` only selects where `push` writes. `RemoteAccessArgs` gains `scope:
Option<&str>` and `max_artifact_size: Option<u64>`.
4. **CI detection** is a pure function of an environment lookup (injected in tests), checked in the
   RFC's order. The provider variable names are checked against current provider docs when written.
5. **Probe results are advisory** (unchanged): the server enforces everything.
6. **No new trust decisions on the client**: nothing here makes a downloaded entry more trusted.

## Batches

1. **Config and scope label** (`omni_configurations`, `omni_cache` helper module, `omni_setup`,
   CLI flags): fields and defaults, back-compat load tests for YAML and msgpack, CI detection and
   label sanitising tests (each provider, PR vs branch, hostile branch names, length, collisions,
   empty).
2. **Client behaviour**: `RemoteAccessArgs` fields and URL, bounded download, `hybrid.rs` push gate,
   target, scope, probe once per run, size skip, warnings. Tests with a local mock server for the
   wire (scope param, bounded body, content-length lie), and `hybrid` tests for push off / read-only
   key / shared without publish.
3. Docs, guide, release note. Prettier, restore the guide's JSON block.

Checks per batch: tests for touched crates, clippy `-D warnings`, fmt, wasm32 worker check. Live
tests and `omni-tests` run in CI.

## Review outcome (Opus, no blockers; all folded in)

- **A failed or oversized read is a miss.** `get_artifact` returns `Ok(None)` with a warning for a
  declared length over the cap, a body that exceeds it, or a truncated stream, so a hostile or
  broken shared read never fails the run. GET gets a total timeout (5 minutes). Tested through
  `get_many`.
- **The server value can only lower the cap:** `min(local.unwrap_or(256 MiB), server.unwrap_or(MAX))`.
  Never allocate from `Content-Length`.
- **Memory:** downloads run behind a semaphore of 8 like uploads; uploads archive through a counting
  writer that aborts at the cap, so a skipped upload never holds the whole archive.
- **Probe:** `Arc<tokio::sync::OnceCell<ProbeOutcome>>` shared by store clones, `get_or_init`
  storing `Unknown` on transport/5xx failure (no retries), 10 second timeout, started with the
  first restore or upload and never awaited by downloads. A `401`/`403` or an invalid probe is
  `Insufficient` (skip uploads), not unknown. The client parses `X-Omni-Principal-Id` and
  `X-Omni-Scope` into `ValidateAccessResult`.
- **Msgpack rules.** New fields are appended at the end of `RemoteCacheConfiguration`, each
  `serde(default)`, never `skip_serializing_if` (the file is a positional array). The back-compat
  test decodes bytes made by a struct of the old shape, through the sync `get_remote_caching_config`
  that `omni_context` uses; old YAML loads and a misspelled field still fails. Downgrade: an older
  omni rejects the new YAML (`deny_unknown_fields`), so the undo is to re-run setup or remove the
  new fields.
- **Both build sites** of `EnabledRemoteConfig` (`cache_store_provider.rs` and `omni_context`
  `create_cache_store`) and the existing `hybrid` tests (`push: true`, plus a separate push-off
  test) are updated.
- **Reads follow the target:** `target: shared` reads shared only; with known scopes lacking
  `write` and `publish`, the isolated GET is skipped. Otherwise isolated then shared.
- **Setup** warns when `--target shared` lacks `publish:artifacts` or `--push` lacks
  `write`/`publish`, sends `scope` in its probe and prints the effective `X-Omni-Scope`.
- **Labels:** the hash covers the raw bytes; a changed or long label is a 55 character prefix, `-`
  and 8 hex; `auto` and `run` are reserved and rejected as literals; `run` outside CI warns and uses
  `default`; run ids are `GITHUB_RUN_ID`, `CI_PIPELINE_ID`, `CIRCLE_WORKFLOW_ID`,
  `BUILDKITE_BUILD_ID`, and `JOB_NAME` plus `BUILD_NUMBER` hashed for Jenkins. Slice 6 builds local
  partition paths from the server-reported label through `segment()`, not the raw client label.
  Detection is tested through an injected lookup. `max_artifact_size` is bytes and rejects 0.
- **Deferred, recorded in section 8:** R2's single conditional write relies on the 32 MiB worker cap
  (server side, not this slice); names `api_key`/`placement` stay instead of `credential`/`tier`.

## Status

Plan reviewed. Implemented in one change: machine-local `push`, `target`, `scope` and
`max_artifact_size` (appended, `serde(default)`, back-compat tested against the old positional
msgpack shape), CI label derivation (`remote_scope.rs`, injected environment), setup flags and
warnings, the probe (`principal_id` and `scope` parsed, 10 second timeout, once per run), bounded
downloads (declared length, chunked, truncated and failing reads are misses), a download semaphore,
a capped upload writer, push and target gating, read order by target and known scopes. Not run
here: the live-service client tests, `omni-tests` and the TS suites (CI). One departure from the
plan: a failing read of either placement is a debug-logged miss, never an error.
