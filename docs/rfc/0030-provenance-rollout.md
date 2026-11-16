# RFC 0030: entry trailer and provenance rollout

Implementation plan for slice 3 of [RFC 0030](0030-remote-cache-trust-model.md) (section 4.3,
section 4.8.4 and 4.8.7). The RFC stays the design authority. This records the decisions the
RFC leaves open, the batches, and what each batch proves.

## Assessment

Large work, High risk: it changes the stored entry format (persistent data), three routes,
the client upload, and the hook contract that operators' SQL depends on. Old entries become
unreadable, so it is hard to undo once data is written in the new format. One implementation
owner. Independent Opus review of this plan, then of each batch before its commit.

## Goal and done

Every remote entry is one object: `[tar.gz][trailer: msgpack][u32 trailer_len][magic]`, and the
server (never the client) writes who stored it, when, and the artifact's size and SHA-256. Done
when:

- an upload whose bytes do not match the client-declared SHA-256 commits nothing and leaves
  the key free, and so does a truncated, oversized or errored upload;
- a stored entry carries server-authored provenance that a client cannot influence;
- `GET` and `HEAD` serve only well-formed entries, and the body returned is exactly the
  artifact (no trailer bytes);
- hooks and the SQL hook queries record the same facts (caller, credential kind, principal,
  namespace, scope, and the SHA-256 on `after`).

Out of scope here: namespaces and scope labels (slice 4: only `shared` exists), client
verification and signing (slice 7), operator provenance/purge routes (slice 8), the
credential/principal response headers (they exist for operators, slice 8).

## Decisions

1. **Trailer encoding.** MessagePack map written with `rmp_serde::to_vec_named` (plain `to_vec`
   writes structs as arrays, which would make adding a field incompatible; a test decodes a
   map with an unknown extra field, and optional fields use `#[serde(default)]`), `trailer_len` as big-endian `u32`, magic `OMNIENT1`. `trailer_len` is at most
   64 KiB, checked on write and on read. Fields: `version` (1), `writer_credential`
   (`kind`, `id`), `writer_principal`, `namespace` (`shared`), `scope` (none until slice 4),
   `stored_at` (RFC 3339 UTC), `size`, `sha256` (lowercase hex), `signature` (none until
   slice 7, opaque to the server). The module lives in `omni_remote_cache_core`; nothing else
   parses a trailer.
2. **Declared hash.** The client sends `X-Omni-Content-Sha256`: 64 lowercase hex characters,
   exactly one header. The server rejects a missing, repeated or malformed value with `400`
   before the hooks run (like a malformed digest) and before reading the body.
3. **Sealing stream.** One adapter replaces `limit_stream`: it hashes the bytes as they pass,
   enforces `max_artifact_size`, and at EOF either appends the trailer (hash equals the
   declared one) or fails. It fails on an upstream error, over the cap, or a mismatch. A shared
   flag records why (cap `413`, mismatch `422`, otherwise `500`), as `exceeded` does today.
   The backend aborts on `Err` and commits nothing (RFC 4.2, already enforced by the storage
   contract tests). The cap applies to artifact bytes, not the object.
4. **Reading an entry.** One helper, `read_entry_tail`, serves `GET` and `HEAD`. It does a
   suffix `get_range` of 64 KiB + 12 bytes. When fewer bytes come back than were asked for, the
   object is that short and `object_len` is the tail length (no `size()` call; R2 already
   spends a `HEAD` inside `get_range`); otherwise it asks the backend for `size()`. It requires
   at least 12 bytes (an S3 `416` fallback returns an empty body), reads the magic and the
   big-endian length from the last 12, requires `0 < trailer_len <= min(64 KiB, tail_len - 12)`,
   parses the trailer, and requires, with `checked_add` in `u64`, that
   `size + trailer_len + 12 == object_len`. `trailer.size` is range-checked before any `usize`
   cast (wasm32 has a 32-bit `usize`). An object that fails any step is a **miss** (`404`,
   logged at `warn` without entry contents). `HEAD` keeps `204` but now costs a ranged read
   (and possibly `size()`) instead of one `exists`. Response headers on `GET`:
   `X-Omni-Namespace`, `X-Omni-Stored-At`, `X-Omni-Content-Sha256` (and `X-Omni-Signature`
   when present, from slice 7).
5. **Body consistency without `If-Match`.** The RFC asks for `If-Match` on both reads, but the
   storage trait has no conditional read, and nothing verifies the hash until slice 7, so
   "a mix is a miss later" is not a mitigation yet. Instead `GET` checks the object it streams
   against the tail it read: it sets `Content-Length: trailer.size`, passes through exactly
   `trailer.size` bytes, holds back everything after them, and at end of stream requires that
   the total equals `object_len` and that the held-back bytes equal the tail's trailer, length
   and magic. Any difference ends the body with an error, which a client with a
   `Content-Length` sees as a failed transfer, not a short success. A re-publish always changes
   the trailer (`stored_at` at least), so every mix is caught, and it does not matter whether
   the tail or the body came from the LRU or the backend. Hashing the body on `GET` as well
   would add CPU on Workers and is a measured follow-up. Recorded in RFC section 8 as a
   deviation from `If-Match`.
6. **Legacy entries and upgrade order.** Entries written before this change have no trailer,
   so they become misses and are rebuilt. They are not deleted (`omni cache purge --legacy` is
   slice 8). Old clients against a new server cannot upload (`400` for the missing hash, which
   the client reports as a warning); new clients against an old server write entries in the new
   format that an old server would serve whole. Upgrade the server first and then the clients,
   and say so in the release note.
7. **List.** `GET /artifacts` still reports the stored object size, which includes the
   trailer. Reporting the artifact size would need a trailer read per entry. Documented.
8. **Hooks.** `api_key_id` is replaced by `caller_id`, `credential_kind`, `principal`,
   `namespace` (`NamespaceKind`: `shared`, and `isolated` reserved so slice 4 adds no contract
   churn), `scope`, and on `after` the `sha256` (empty for operations with no entry). The SQL
   hook queries bind 11 parameters for `before` and 13 for `after`, in the RFC's order; the
   startup `validate_queries` counts change and operators rewrite their SQL.
9. **Memory.** Core adds no buffer of its own. The backends spool (S3), buffer (R2's multipart
   `BytesMut`) and, through the LRU decorator, may keep the sealed entry (up to its
   `max_cacheable_size`, which now counts the object, not the artifact). On Workers the 32 MiB
   cap bounds the peak at roughly twice the entry.
10. **Hook `sha256`.** `after` carries the computed hash for a stored entry and the trailer's
    hash for a `GET` hit (cheap, and useful for audit); it is empty for `HEAD`, `409`, misses
    and failures. `after_response` takes it as a parameter.

## Batches

1. **Trailer and sealing stream.** Pure, additive, nothing calls them yet. Trailer encode,
   decode and the read-side checks of decision 4; the sealing stream with the cap, hash,
   mismatch and error behaviour and an explicit flag precedence (cap `413` before mismatch
   `422`). It emits exactly one terminal `Err` and nothing after it, and appends the trailer
   only after the inner stream ends and the hash matches. Tests: round trip; a map with an
   unknown extra field; every malformed-tail case (short object, fewer than 12 bytes, no magic,
   `trailer_len` zero, past the object, over 64 KiB, truncated msgpack, wrong version); a
   trailer that lies about `size` (including values that overflow `usize` or `u64` addition); an
   artifact whose own last bytes are the magic; the stream's cap boundary (exactly the cap and
   one over); mismatch; upstream error mid-stream; empty artifact; chunk-boundary independence;
   no chunk after an `Err`. Undo: delete the modules.
2. **Routes, client and fixtures.** `PUT` seals, `GET` and `HEAD` use `read_entry_tail`, `GET`
   applies the consistency check of decision 5, the client sends `X-Omni-Content-Sha256`, and
   legacy entries are misses. The client computes the hash in
   `omni_remote_cache_client::put_artifact` (the single wire concern, adding `sha2` and `hex`),
   inside a `spawn_blocking` so a 256 MiB archive does not stall the runtime; the bytes are
   already resident and retries reuse the same `Bytes`. **Every PUT caller moves in this
   batch:** the TypeScript service specs, the service tests (`integration`, `immutability`,
   `authentication`, `hooks`, `sql_repository`) and the mock servers in the `hybrid.rs` tests,
   which now assert the header. Tests through the real routes with the local backend: tamper
   cases (declared hash wrong, body truncated, two headers, uppercase hex, a `Content-Length`
   lie); an artifact whose own tail resembles the magic; an empty artifact sealed and read
   back; `409` leaves the first entry's provenance intact; provenance is server-authored
   (client-supplied provenance headers are ignored); `GET` returns exactly the artifact bytes
   and `Content-Length`; `HEAD` agrees with `GET`; a legacy object and an object with a valid
   looking tail but invalid msgpack are misses; a tail that disagrees with the body stream
   aborts the body; the same entry served from the LRU and from the backend after eviction is
   byte-identical; the cap still gives `413`. Docs: the new header, `400`, legacy misses and the
   list size note, in the site guide and the OpenAPI annotations. Undo: **a rollback needs the
   new-format entries purged** (or a separate container), because an old server would stream the
   whole object, and a single-member gzip reader may ignore the trailing bytes and restore from
   it. Revert server and client together.
3. **Hooks contract.** `HookContext`, `HookOutcome`, `after_response` (which batch 2 prepares
   for the new `sha256` parameter), the SQL hook parameters and counts, the site guide and RFC
   0027 examples, and every `api_key_id` reference (the D1 and SQL executors, the worker and the
   TypeScript fixtures). Tests: every operation records the new facts, `after` carries the hash
   as decided, the SQL queries bind in order, and a query written for the old 7 or 8 parameters
   is rejected by the databases that need the exact count; the message names the counts ("uses N parameter(s) but M are bound"), and only a Postgres or MySQL run in CI can show it, because SQLite accepts fewer. 11 and 13 parameters are
   far below D1's limit. Undo: revert; operators restore the old SQL. **Decided while building:
   the RFC's order is not used, and append-only is now the standing contract for hook parameters.** It would move `outcome` in the `after` query, and SQLite accepts
   a query that uses fewer parameters than are bound, so an old operator query would keep running
   but silently record the caller id in the outcome column. Every existing position is kept and
   the new parameters are only appended (`before`: caller_id, credential_kind, principal,
   namespace, scope; `after`: outcome, caller_id, credential_kind, principal, namespace, scope,
   sha256). An old query then means exactly what it did, and Postgres, MySQL and D1 (which need
   the exact count) still reject a stale query (at startup natively, on first use on D1).

Checks per batch: the unit and integration tests for the touched crates, clippy with
`-D warnings`, formatting and the wasm32 worker check. The `test:service:*` suites run in CI.

## Risks and how they are covered

- **Trailer confusion.** An artifact is attacker-influenced bytes and could end with the magic.
  The reader always takes the last 12 bytes of the sealed object, so the real trailer wins and
  there is no ambiguity; a test stores such an artifact.
- **Writer forging provenance.** Provenance comes only from the `Caller`; no request field,
  header or body byte reaches the trailer except `signature` (later) and the declared hash.
- **Partial commit.** The commit is the backend's conditional single write; a failed seal
  returns `Err` before it. Backend tests already cover abort on `Err`.
- **Operator SQL breakage.** Counts are validated at startup, so a stale query fails loudly.

## Status

Plan reviewed by Opus (four blockers, all folded in above: the body-consistency check replacing
`If-Match`, the fixtures and clients that move with the header, the rollback and upgrade order,
and `to_vec_named`). RFC section 8 deviations to record: no `If-Match`, and the credential and
principal response headers deferred to slice 8.

- **Batch 1 (trailer and sealing stream): implemented and independently reviewed by Opus (no
  blockers).** New `entry` module in `omni_remote_cache_core` (`rmp-serde` is the only new
  dependency, already in the workspace). `Trailer::encode` and `Trailer::parse_tail` cover
  decision 1 and the read side of decision 4; `seal` is the sealing stream of decision 3 with a
  `SealHandle` so a route can tell `413`, `422` and `500` apart after a backend reports only
  that the write failed. Review follow-ups applied: the trailer region must be exactly one
  trailer (trailing bytes are `Malformed`, so there is one canonical encoding for the `GET`
  held-back comparison and any later signature), the tail may not be longer than the object,
  `Parsed::matches_tail` holds the `GET` consistency check in one tested place, and the handle's
  contract is documented: read it only after the backend call returns, `None` after a failed
  write means the backend failed, and a mismatch is known only after every artifact byte was
  forwarded, so each backend's batch-2 tests must show it commits nothing before the stream's
  end. Nothing calls the module yet, so there is no behaviour change. 29 unit tests.
- **Batch 2 (routes, client and fixtures): implemented and independently reviewed by Opus
  (one blocker, fixed).** The review found that the end-of-stream check of `checked_body` never
  reached an HTTP/1 client: hyper stops pulling once it has sent `Content-Length` bytes, so the
  check ran after the response had already succeeded. `checked_body` now holds back the last
  artifact byte until the object has been checked; one test models an HTTP/1 reader and one
  drives a real socket with `reqwest`, and both fail if the byte is released early (verified by
  reverting). Other follow-ups: a test records that artifact-region corruption is served until
  the client verifies (slice 7), and RFC section 8 now lists the `409`/`422` backend difference
  and the `HEAD` cost.
  - `PUT` validates `X-Omni-Content-Sha256` after the declared-length `413` check and before the
    hooks, then seals the body (`seal`, replacing `limit_stream`); a failure maps to `413`
    (cap), `422` (mismatch) or `500` through the handle, read after the backend returns.
  - `GET` and `HEAD` share `read_entry`; `GET` streams through `checked_body` with
    `Content-Length: size` and sends `X-Omni-Namespace`, `X-Omni-Stored-At`,
    `X-Omni-Content-Sha256` (and `X-Omni-Signature` when present).
  - The client hashes the archive in `spawn_blocking` and sends the header.
  - The `limit_stream` unit tests were replaced, not dropped: the seal tests cover the exact
    limit, one over, a single oversized chunk and nothing emitted after the error, and
    `provenance.rs` covers the cap through the route.
  - Fixtures moved with the header (service tests, the TypeScript spec, the `hybrid.rs` upload
    mock, which now asserts the declared hash equals the body's). Four immutability tests that
    compared raw stored bytes now compare the artifact inside the sealed object, and the
    conflict test also checks the stored object is unchanged byte for byte.
  - New: `entry_read.rs` (reader against the memory backend and the LRU, a swapped, longer,
    shorter or truncated object, every chunk boundary) and `provenance.rs` (13 route tests:
    server-authored provenance, forged headers ignored, wrong or malformed hash, a failing
    body, exact `GET` bytes and headers, trailer lookalikes, empty and large artifacts, `HEAD`
    agreement, tampered and legacy objects, `409` leaves provenance intact, list sizes, the cap).
  - The TypeScript tests (including new ones for `400`, `422`, headers and `HEAD`) run in CI
    only.
- **Batch 3 (hooks contract): implemented and independently reviewed by Opus (no
  blockers; the append-only order confirmed correct on every database).** Follow-ups applied:
  append-only stated as the standing contract, the Postgres and MySQL parameter notes
  corrected, and the hash test made independent of timing. `HookContext` drops
  `api_key_id` for `caller_id`, `credential_kind`, `principal`, `namespace` (`NamespaceKind`)
  and `scope`; `HookOutcome` gains `sha256` (the stored hash for `Stored`, the entry's for a
  single-artifact `Hit`, otherwise none, enforced in `after_response_with`). The SQL hook queries
  bind 11 and 13 parameters in the append-only order above. Tests: the exact bound lists, that the
  older seven and eight parameters are a prefix, the declared counts, every operation's `after`
  hash (stored, 409, hit, miss, `HEAD`, list, delete), a non-API-key caller's kind and principal
  reaching the hooks, a real SQLite database recording the new columns, and an old-shape query
  still recording what it did. The D1 target's queries and usage table moved with the counts (D1
  needs every parameter referenced); that and the TypeScript assertions run in CI only.
