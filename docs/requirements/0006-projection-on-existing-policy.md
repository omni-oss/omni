# Requirements: Projection `on_existing` policy, fatal collisions, and a pre-apply check

- **RFC:** [0006-projection-on-existing-policy](../rfc/0006-projection-on-existing-policy.md)
- **Status:** Accepted
- **Date:** 2026-08-29

## Summary

A projection's per-destination option is renamed from `on_collision` to `on_existing`, because it only ever governed one thing: what to do when a file is already sitting at the destination. Its default flips from `backup` to `error`, so a projection no longer silently moves a foreign file aside. This change also splits two situations the old option ran together. A file already on disk is prior state and stays configurable through `on_existing`. Two of the author's own links fighting over one path is a self-contradictory plan, so it is always a fatal error that no policy value can override. A single read-only pass enforces both rules across every source before the first write, so a run either applies fully or aborts with a complete conflict list and an untouched filesystem.

## Goals

- Rename the `on_collision` projection option to `on_existing`, with no compatibility alias.
- Change the default of `on_existing` from `backup` to `error`.
- Make a plan collision always fatal, independent of `on_existing`: two links resolving to one destination, or one destination nested strictly inside another projected directory link.
- Detect every collision and every existing-file conflict across all sources before any filesystem write, and abort if any exist.
- Keep a routine re-sync of omni-owned links a no-op, never flagged as a conflict.
- Report all conflicts in a single aggregated error.
- Report the same conflicts under `--dry-run` without mutating anything.

## Non-goals

- A full transactional or rollback sync. The pre-apply pass prevents partial writes for collisions and for the `error` existing-file case, but a failure that surfaces mid-write (a permission error, or a file appearing after the check) stays best-effort.
- Changing what `skip`, `overwrite`, or `backup` do for a file that already exists. Only the option name and the default change.
- An intentional override mechanism to let two rules deliberately target one destination.
- Any change to routing or matching.

## Requirements

### Configuration

REQ-01 The projection common option previously named `on_collision` must be named `on_existing`.
REQ-02 `on_existing` must accept the values `skip`, `overwrite`, `backup`, and `error`.
REQ-03 When `on_existing` is omitted, its default must be `error`.
REQ-04 A config that uses the key `on_collision` must fail to parse; there is no alias to the new name.
REQ-05 The generated config schema must advertise `on_existing` on every projection strategy arm, and must not mention `on_collision`.

### Behavior

REQ-06 When a non-owned file already exists at a destination whose `on_existing` is `error`, the sync must fail and report that destination.
REQ-07 The `skip`, `overwrite`, and `backup` values must behave for an existing file exactly as they did before this change.
REQ-08 A destination the ledger records as omni-owned for its source, with an unchanged pin, must not count as an existing-file conflict, so a repeated sync stays a no-op.
REQ-09 An omni-owned destination whose pin has changed must be re-pointed, not treated as a conflict, whatever `on_existing` says.
REQ-10 Two or more planned links anywhere in the run that resolve to the same destination must be a fatal collision under every `on_existing` value, including `overwrite`.
REQ-11 A destination that sits strictly inside another planned directory-link destination, across different projections or sources, must be a fatal collision.
REQ-12 Within a single projection, a candidate link whose destination nests strictly inside another candidate directory link's destination must still be pruned (shallowest wins), not errored.
REQ-13 Two rules that produce an identical planned link (same source, same destination, same link kind) must collapse to one link and must not be reported as a collision.
REQ-14 Both the collision check and the existing-file check must run over the full plan of every configured source before the first filesystem write.
REQ-15 If either check finds any conflict, the run must abort having written nothing.
REQ-16 A single error must list every collision and every existing-file conflict found in the run, rather than failing on the first one.
REQ-17 A `--dry-run` must run both checks and report any conflicts without mutating the filesystem.
REQ-18 A targeted `--source=X` run must scan, check, and apply only that source, leaving every other source's links and checkout untouched.

### Breaking changes

REQ-19 Before this change a projection used the key `on_collision`; after this change it must use `on_existing`, and a config still using `on_collision` is rejected at load. Affects: anyone who set the option explicitly.
REQ-20 Before this change the option defaulted to `backup`, silently moving a foreign file aside; after this change it defaults to `error`, so a foreign file at a destination fails the sync unless the author sets `on_existing` to `backup`, `overwrite`, or `skip`. Affects: configs that relied on the implicit default and have a foreign file at a destination.
REQ-21 Before this change two rules aimed at one destination resolved through the policy, so `overwrite` or `backup` picked a last-write winner; after this change that is always a fatal collision. Affects: configs that used two rules on one destination as a deliberate override.
REQ-22 Before this change a destination nested inside another projected directory link across projections or sources reached apply and wrote through the outer link, silently landing inside a source when that link was a symlink; after this change it is a fatal collision. Affects: configs with cross-source or cross-projection nested destinations.
REQ-23 Before this change an `error` conflict on a later source could leave earlier sources already materialized; after this change nothing applies if any source conflicts. Affects: multi-source syncs that previously left a partial result on failure.

## Migration / upgrade path

Rename every `on_collision` key to `on_existing`; an un-migrated config fails to load rather than misbehaving. If a projection relied on the old silent-backup default and a foreign file can sit at its destination, add `on_existing: backup` (or `overwrite`, or `skip`) to keep that behavior. If a config deliberately pointed two rules at one destination and leaned on `overwrite` to pick a winner, that no longer works and must be rewritten so each destination has one owner; there is no drop-in replacement. Routine re-syncs of omni-owned links need no action and stay no-ops.

## Technical Details

The rename covers the option key and its policy enum, with no serde alias, which is safe because the projection common config already rejects unknown fields, so an old key fails loudly at parse. The default flip is a one-attribute move on the policy enum. The two checks share nothing with each other: the collision check is a pure grouping over the accumulated planned destinations with no filesystem access, and runs after the existing intra-projection nested pruning so only genuine survivors count; the existing-file check stats each `error`-policy destination and reuses the ledger ownership test through a predicate shared with the per-link apply path, so the two cannot drift. The per-link `error` arm stays as a fallback for a file that appears between the check and the write. Making the pass whole-run requires planning every source before applying any, so the single per-source plan-and-apply pass is split into a plan phase and an apply phase, and the driver accumulates all plans, runs both checks, then applies. Conflicts are reported through a structured aggregated error variant so the CLI can format them and tests can assert on counts.

## Out of scope

- Any rollback or undo for a failure that surfaces mid-write.
- An explicit override or rule-precedence mechanism to replace the retired overwrite-wins trick.
- Any compatibility alias for the retired `on_collision` key.
- Any change to `skip`, `overwrite`, or `backup` semantics for an existing file.
- Any routing or matching change (tracked in RFC 0005).
