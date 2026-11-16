# RFC 0030: isolated entry expiry (slice 8, reduced)

Slice 8 of [RFC 0030](0030-remote-cache-trust-model.md) originally covered `omni cache provenance`,
`omni cache purge` (by credential, principal, signing key, digest, and `--legacy`) and isolated expiry.
Decision (maintainer): cleanup of old artifacts is a server or storage-provider concern, no production
remote cache exists yet so there is no legacy layout to purge, and a client-side purge command adds a
second, confusingly named cleanup next to `omni cache prune`. This slice therefore builds only expiry.

## Built

- `omni_remote_cache_core::sweeper::sweep_isolated(storage, cutoff, max_objects)` removes isolated entries
  whose trailer `stored_at` is before the cutoff. It walks `isolated/` with `list_prefix`, reads each
  trailer tail, and deletes only entries with the exact key shape
  `{tenant}/{org}/{ws}/{env}/{principal}/{scope}/{digest}` and a valid trailer. Wrong shapes and objects
  with no valid trailer are skipped and counted, never deleted. Shared entries are never touched. It has no
  runtime dependency, so it also compiles for the worker target.
- The service runs it hourly for the local-disk and memory backends only (`--isolated-ttl-days`, default 7,
  `0` disables, env `OMNI_REMOTE_CACHE_SERVICE_ISOLATED_TTL_DAYS`). Each pass is bounded to 100,000 objects so a
  large store is worked through over several passes, and a failed pass only logs. Deletion goes through the
  service's storage handle, so the LRU decorator stays consistent.
- S3 and R2 use a bucket lifecycle rule on the `isolated/` prefix (the namespace is the first key segment);
  the guide ships the rule.

## Deferred (not built, by decision)

Server purge routes, the provenance route, `credential`/`principal` response headers, `omni cache purge`,
`--legacy`, a local `omni cache clean` of unreadable partitions. Recorded in RFC section 8. Incident response
for a leaked credential or revoked signing key is: revoke the credential, rotate the signing key (shared
entries signed by a removed key are misses), and remove affected objects with the storage provider's own tools.

## Tests

Sweeper unit tests (old isolated entries removed; shared, junk and odd paths survive; a pass can be
bounded). Service wiring is exercised by compilation; the loop itself is intentionally trivial.
