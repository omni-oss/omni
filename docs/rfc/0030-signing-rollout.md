# RFC 0030: signing and verification rollout

> Note: mentions of `omni cache purge` or `--legacy` below are superseded by `0030-operator-tools-rollout.md` (slice 8 was reduced to isolated-entry expiry).

Plan for slice 7 of [RFC 0030](0030-remote-cache-trust-model.md) (section 4.5, 4.4's `trusted_keys` and
`verify`, 4.8.5, 4.8.8). The RFC stays the design authority.

## Assessment

Large work, High risk (it is the control that stops a compromised server or storage from feeding
unsigned or forged bytes to a trusted run, and it adds cryptography and committed policy). Plan,
independent Opus review of the plan, then two batches each reviewed before commit:

- **7a: primitives and policy.** Ed25519 signing module, key ids, the signed message, `keygen`, the
  committed workspace policy (`trusted_keys`, `verify`), the machine-local `signing_key` and `verify`,
  the never-loosen resolution, and the server accepting `X-Omni-Signature` on shared uploads.
- **7b: client behaviour.** Sign on shared upload, verify before extraction, the modes, the real trust
  epoch inputs, `verified_by` in markers and the `require` local-hit rule, the publisher check after
  a shared `409`.

## Goal and done

- `omni cache keygen` prints a new Ed25519 keypair and its key id (first 16 hex of SHA-256 of the
  public key). The private key is shown once and never stored or logged by omni.
- Committed `workspace.omni.yaml`: `remote_cache.signing.trusted_keys` (`id`, optional `name`,
  `public_key` base64) and `verify: auto | off | warn | require`. An entry whose `id` is not the id
  derived from `public_key` is a configuration error. Duplicate ids are errors.
- Machine-local remote cache file (appended fields, same msgpack rules as slice 5):
  `signing_key {id, private_key_env | private_key_file}` and a local `verify` that may only tighten
  (`off < warn < require`) plus an optional local narrowing of `trusted_keys` to a subset. `auto`
  resolves to `require` when the workspace set is non-empty, else `off`, before the stricter-of merge,
  and is never re-evaluated after narrowing.
- Server: `PUT target=shared` accepts at most one `X-Omni-Signature` (`<key_id>:<base64>`, visible
  ASCII, at most 256 bytes) and stores it in the trailer; an invalid value is `400`; the header on an
  isolated `PUT` is `400`. The server never verifies (it has no keys); `GET` already returns it.
- Client (7b): shared uploads are signed over
  `len||"omni-cache-v1" ‖ tenant ‖ org ‖ ws ‖ env ‖ "shared" ‖ digest ‖ sha256(archive)` (u32 big-endian
  length prefixes). A shared upload with `trusted_keys` configured and no `signing_key` is refused
  client-side with a clear warning. Shared hits are verified before any byte reaches disk: recompute
  `sha256` of the downloaded bytes, require it to equal `X-Omni-Content-Sha256`, find `key_id` in the
  effective trusted keys, verify. `require` failures are misses, `warn` restores and reports, `off`
  skips. Isolated hits are never verified (unsigned by design; only untrusted runs read them).
- The marker's `verified_by` is the key id that verified the entry (`none` otherwise). Under
  `require`, a local hit on a `shared`-marked entry is reused only if `verified_by` names an effective
  trusted key. The trust epoch uses the effective key ids and verify mode (it is a constant today).
- After a shared `409` a publisher fetches the existing entry and verifies it; a failure is a warning
  naming the digest and the purge remedy (purge is slice 8).

Out of scope: `omni cache purge` and `provenance` (slice 8), HSM/KMS, key revocation lists, multiple
signatures, verifying isolated entries.

## Decisions

1. **Where the code lives.** A `signing` module in `omni_remote_cache_client` (it already owns the
   wire contract and is wasm-free); config types in `omni_configurations`; the server only stores an
   opaque string.
2. **Dependencies.** `ed25519-dalek` 2 (std only, no rand feature; key bytes come from the OS RNG via
   `rand` 0.10) and `base64`, both added to the workspace. `Cargo.lock` changes are part of the diff.
3. **Key id binding.** The id is derived, never trusted from config; a mismatch is a hard error at
   config load. A removed id is not reused (documented).
4. **Private key handling.** From an environment variable or a file path (`0600` expected on Unix,
   warned otherwise); never part of the committed config or the encrypted-or-plain remote config
   file; held in a type with a redacting `Debug` and zeroized on drop; never logged. Registering the
   value with task log redaction (RFC 0022/0026) is recorded as a limit: a key in the environment is
   visible to child processes, so publish jobs should expose it only to the signing step.
5. **Verify scope.** Only `shared` placement hits; the message binds tenant, org, ws, env, the
   namespace `shared`, the digest and the archive hash, so an entry cannot be replayed elsewhere.
6. **Mode failure semantics.** Unsigned, unknown key, bad signature, missing or mismatching hash are
   all rejections of the same kind; a short diagnostic names the digest and the reason, never the
   signature bytes.
7. **Client API change.** `get_artifact` returns an artifact with its bytes, `X-Omni-Content-Sha256`
   and `X-Omni-Signature` (a small struct) instead of bare bytes; existing callers and tests move.
8. **Never loosen.** Local config cannot add a trusted key, lower the mode, or clear a workspace
   policy; a machine nobody configured still verifies whatever the repository says.

## Batches

1. **7a**: signing module (message, key id, sign, verify, keygen) with test vectors, tamper and
   length-prefix tests; config types, validation and resolution with a table of never-loosen cases;
   server header handling and tests (valid, oversized, bad characters, duplicate, on isolated);
   `omni cache keygen`; dependency additions.
2. **7b**: client changes above, the epoch, markers' `verified_by`, the `require` reuse rule, the 409
   check, end-to-end tests against a mock server (signed ok, unsigned miss in `require`, wrong key,
   tampered bytes, replay under another digest or tenant, `warn` restores, `off` skips), docs.

Checks per batch: tests for touched crates, clippy `-D warnings`, fmt, wasm32 worker check.

## Risks

- A verification bug that accepts a forged entry (covered by negative tests for each field of the
  message and for every rejection reason).
- Key handling leaks (redacting `Debug`, no logging, tests that the formatted config never contains
  the key).
- Policy loosening through local config (table-driven tests).
- Cold caches when the epoch changes (documented).

## Open questions for review

- Is `ed25519-dalek` 2 with `std` only acceptable, and is deriving key bytes from `rand` 0.10 right?
- Should the publisher's post-`409` check read the existing entry (needs `read`), or only warn when
  it can?
- Is the 7a/7b split right, and can 7a ship alone (policy without enforcement)?
- Anything missing from the verification rules for a malicious server (e.g. sizes, header duplicates,
  the hash header absent)?

## Review outcome (Opus; four blockers, all folded in)

- **B1 dependencies.** `ed25519-dalek` with `std` and `zeroize` (no `rand_core`); key seeds come from
  `ring`'s `SystemRandom` (already a workspace dependency, used by `omni_setup`), zeroized, and the
  printed key is a `Zeroizing<String>`.
- **B2 setup flags.** `omni cache remote setup` gains `--signing-key-id`, `--signing-key-env` or
  `--signing-key-file`, `--verify` and `--trusted-key` (repeatable). The local fields are appended
  to the msgpack struct with `serde(default)`: `signing_key`, `verify`, `trusted_keys` (an `Option`:
  absent is no narrowing, `Some([])` trusts none).
- **B3 policy path.** One resolver (`resolve_signing`) builds the policy from the workspace's
  `remote_cache.signing` and the local settings; `EnabledRemoteConfig::from_configuration` takes the
  workspace section and both call sites pass it. An invalid policy fails closed (every shared entry
  rejected, shared uploads refused) and logs an error. `WorkspaceConfiguration` is
  `deny_unknown_fields`, so older omni binaries refuse a workspace file that has `remote_cache`: they
  fail closed, and the release note says so.
- **B4 key handling.** `EnabledRemoteConfig` carries only a reference (`id`, env var or path). The
  key is loaded lazily on the first shared upload into a `SigningHandle` (redacting `Debug`, no
  `Serialize`, zeroized), checked against the configured id, read only from `std::env` or a file,
  never from omni's `.env` loader.
- **Should-fix folded in:** `verify_strict` and weak-key rejection; the exact wire grammar (16
  lowercase hex id, 88 character padded standard base64) enforced by client and server; duplicate
  `X-Omni-Signature` or `X-Omni-Content-Sha256` headers rejected and the message built from the hash
  the client computed; one helper for the `env` value used in URL and message; hashing and verifying
  on the in-memory bytes with no second copy; strict marker parsing and the `require` reuse rule
  applied where the isolated check already is; the domain label is `omni-cache-entry-sig-v1`; the
  private key environment variable is removed from task environments; narrowing to an id the
  workspace does not trust warns and is ignored; the epoch uses the resolved mode and the narrowed
  key ids. Registering the key with log redaction is a recorded deviation.
- **Split.** 7a is primitives and policy with no behaviour change (the epoch stays constant); 7b
  changes the epoch, markers and enforcement together and carries the server header handling, so the
  producer and consumer are reviewed together.

## Status

7a implemented (signing module, policy types and resolver, setup flags, `omni cache keygen`).
7b implemented: the server accepts one well formed `X-Omni-Signature` on shared uploads (400 on isolated,
malformed or duplicate); the client sends it, reads provenance headers once (duplicates make the entry
unverifiable), signs shared uploads (refusing them when keys are trusted and none is held), verifies
shared hits on the in-memory bytes before extraction, applies the three modes, records `verified_by`
only under `require`, discards unverified or untrusted-key entries under `require`, puts the resolved
mode and key ids in the trust epoch, and checks an existing shared entry after a `409`.
The key's environment variable is removed from every child process (`omni_process::protect_env_var`,
registered by the pipeline). Not done (recorded in RFC section 8): registering the key with task log
redaction; the `omni-tests` end-to-end compromise scenario. Opus's pre-commit review found that shared
uploads were not refused when `require` left a machine with no trusted key (an invalid or narrowed
configuration): fixed, `require` without a signing key now refuses shared uploads.
