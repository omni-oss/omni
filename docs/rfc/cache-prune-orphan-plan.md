# Plan: worktree-aware orphan and stale cache pruning (#136)

Status: scope approved; verify discovery and hashing before wiring deletion.

Progress: checkout discovery, peer cache resolution, shared inventory,
selection and confirmation are implemented. The 163 scoped CLI end-to-end
tests cover different digests in linked checkouts, custom cache paths, a peer
that uses a different cache, disabled tasks, failed loads, dependency hashes,
the retired flag, and an inventory change during the confirmation prompt.
Independent review and platform CI are required before merging.

## Goal and scope

`omni cache prune --orphan` selects local cache entries whose `project#task`
is no longer defined in any linked checkout using the same cache.
`omni cache prune --stale` replaces `--stale-only` and keeps any entry whose
digest is current in at least one of those checkouts. Neither mode deletes
remote entries. Ordinary name, age, and size pruning and the cache format
stay unchanged.

This is large, high-risk work: checkout enumeration and workspace loading
feed two deletion policies exposed through CLI, Rust API and MCP. Main
unknowns are strict inventory completeness and how to compute comparable
digests in sibling checkouts without installing sources or running tasks.
Reverting the feature does not restore removed entries; they must be rebuilt.
Separate checkout/inventory work from selection and interface work after
settling their contract, and independently review the integrated result.

## Proposed decisions

- Enumerate the primary and linked Git checkouts, map the current Omni
  workspace's relative path into each checkout, and compare their resolved
  cache roots. Load only relevant workspaces. Fail closed if a relevant
  workspace or its cache sharing cannot be evaluated; never infer that an
  unreadable checkout has no tasks.
- For `--orphan`, build the inventory from all finalized projects and their named task
  definitions, including disabled tasks. Do not build an execution plan or hash
  inputs or load environment files. Do not count declaration-only projects
  loaded solely as templates.
- For `--stale`, use each checkout's own configuration, inputs, and dependency
  digests with the existing current-digest calculation. A stale entry must
  have at least one current task definition, and no matching current digest.
  Keep an entry when any digest matches or a relevant definition cannot
  produce a digest. Use the executor's cache path expansion and transitive
  cache eligibility rules. Resolve a valid project pattern separately in each
  checkout, where it may match no projects.
  Abort deletion if computing a required digest fails. Environment loading is
  read-only: command substitutions are rejected, not evaluated or hashed
  literally. This still measures
  freshness for the prune invocation's environment, not every environment or
  argument combination future builds might use.
- Replace the CLI flag `--stale-only` with `--stale` and provide no alias.
  Document the breaking CLI rename so scripts can migrate to `--stale`.
  Rename the Rust/MCP field to `stale`; the old `stale_only` name stays as a serde alias for deserialization.
- A custom cache path works without an override from a discoverable Git
  worktree. Compare resolved paths rather than requiring the default layout.
  For an explicit invocation-level path, conservatively consider every linked
  checkout a possible consumer: Git cannot recover flags passed to past runs.
  The guarantee covers discoverable linked checkouts only, not another
  repository or machine using the same directory. For an undiscoverable
  checkout with a custom cache path, refuse deletion in these modes. Do not
  add an override flag.
- Exact names, patterns, age and size narrow the candidate list but do not
  narrow the inventory. Reject `--orphan` with `--dir` or `--meta` because absent
  definitions have no current directory or metadata. Reject `--orphan` with
  `--stale`: the two conditions are mutually exclusive
  and combining them as ordinary AND filters would never match.
  Declare the conflict in the CLI parser so the combination fails before
  workspace loading or cache access. Enforce it at the common operation
  boundary too, so API and MCP callers receive the same rejection.
- Preview uses the existing listing; deletion uses the existing partition lock
  and candidate removal path. Re-check safety and candidate eligibility after
  interactive confirmation, before deleting; skip an entry whose task now
  exists and do not add entries that weren't previewed. Do not sweep unrelated
  staging directories as part of orphan-only deletion.

Do not infer ownership from who wrote an entry: that does not show which
checkouts can still use it. Do not add persistent ownership metadata.

## Shared inventory

List cache entries once per partition, not once per checkout. Reduce the
worktree observations into an in-memory inventory keyed by `(project, task)`.
For each key, record whether it is defined, the union of known current
digests, and whether any definition lacks a digest. `--orphan` needs only
definition membership; it does not compute hashes. `--stale` computes
digests for candidate tasks and any dependencies needed to hash them.

Classify an entry as orphaned if the key is absent from the complete
inventory, current if any recorded digest matches, and stale if the key is
defined and all relevant definitions yielded digests but none matches.
Otherwise keep the entry as indeterminate. A checkout load or digest
calculation error aborts the operation instead of contributing an empty
inventory. Never use an incomplete inventory to authorize deletion.

## Build and verification

1. Verify strict checkout enumeration, cache-root resolution, and workspace
   loading on a two-worktree fixture, including failed loads and a checkout
   configured to use a different cache. Stop if incomplete discovery cannot
   be distinguished from a complete inventory.
2. Add the selection and deletion policy and expose it through CLI/API/MCP.
   Test removed and disabled tasks, exact selectors, dry run, rejection of
   the retired `--stale-only` spelling, and eligibility changes after preview. With different digests
   in two linked worktrees, keep both current entries and remove an entry
   stale in both. Likewise, only remove an orphan absent from both.
3. Run scoped cache, context, API and MCP tests, CLI e2e checks and platform CI
   including native Windows. Get independent read-only review before publishing.
   Rollback is a normal revert; no migration is needed.
