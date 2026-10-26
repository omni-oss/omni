# Requirements: Managed ignore sync (`omni ignore sync`)

- **RFC:** [0012-managed-ignore-sync](../rfc/0012-managed-ignore-sync.md)
- **Status:** Accepted
- **Date:** 2026-09-11

## Summary

omni writes files into the workspace that should not be committed: the projection
links from RFC 0001/0010 and its own state under `.omni/`. Keeping `.gitignore` in
step with all of that is the user's job today, done by hand, and it drifts the
moment a projection is added, renamed, or a meta bundle grows a member. This change
adds `omni ignore sync`, a command that maintains one fenced, omni-owned block in
each configured ignore file listing every path omni manages. The projections
contributor reads the projection ledger, so the ignored set always matches what
sync actually linked, with no second source of truth.

## Goals

- Maintain one fenced, omni-owned region in each configured ignore file (default
  `.gitignore`, `.ignore`, and `.omniignore`, all at the workspace root) that lists
  every path omni manages.
- Feed that region from named contributors toggled in `workspace.omni.yaml` under
  `ignore.sources`: `projections` (reads the ledger) and `internal` (omni's own
  `.omni/` state).
- Let the target files be configured under `ignore.files`, defaulting to the three
  files above when absent or empty.
- Rewrite only the fenced region on every run, preserving all hand-written lines
  before and after it verbatim.
- Be idempotent and deterministic, so re-running never churns the diff.
- Offer `--dry-run` and `--check` (for CI) alongside the default apply, plus a clean
  teardown that removes the block.

## Non-goals

- No automatic invocation from `projection sync`; the command is run explicitly.
- No per-directory ignore files and no `.git/info/exclude` mode in v1.
- No per-subsystem `ignore.sources` toggles in v1; `internal` is one coarse toggle.
- No contributor for generators, tools, or the cache in v1.
- No rewriting or reordering of the user's own ignore rules; omni owns exactly one
  block per file and nothing else in it.

## Requirements

### Configuration

REQ-01 The workspace configuration must gain an optional `ignore` key holding a
`files` list and a `sources` map.

REQ-02 An absent or empty `ignore.files` must resolve to `.gitignore`, `.ignore`,
and `.omniignore`, all at the workspace root.

REQ-03 `ignore.sources` must expose two boolean toggles, `internal` and
`projections`, each defaulting to on.

REQ-04 An absent `ignore` key must behave exactly as the full default (all three
files, both sources enabled).

REQ-05 The system must write the same rendered block to every configured file.

### Behavior

REQ-06 The system must maintain exactly one fenced region per configured file,
delimited by a `# @@omni-managed:begin` line and a `# @@omni-managed:end` line, and
must locate that region by matching the fence tokens.

REQ-07 The end fence must carry a content hash of the rendered lines, and `--check`
must recompute it so a hand-edit inside the fence is detectable.

REQ-08 The system must rewrite only the fenced region and preserve every line before
and after it byte-for-byte.

REQ-09 Identical inputs must produce a byte-identical block, so a re-run with no
changes writes nothing.

REQ-10 The block must not be globally sorted; determinism must come from a fixed
contributor order plus deterministic per-contributor emission.

REQ-11 A negation line (for example the `.omni/sources/*/lock.json` keep-rule) must
be emitted after the base pattern it re-includes and must never be reordered ahead
of it.

REQ-12 A concrete workspace-relative path must be anchored with a leading `/` and
must have gitignore-significant characters (`#`, `!`, `[`, trailing spaces) escaped
so a literal path is never read as a comment, negation, or character class.

REQ-13 The `projections` contributor must emit one anchored pattern per projection
ledger `dest`, sorted, with no directory-versus-file distinction.

REQ-14 The `internal` contributor must emit a surgical, ordered set of `.omni/`
patterns (not a single wholesale `/.omni/` line) that preserves the committed
`lock.json` keep-rule.

REQ-15 `.omni/` patterns must be root-anchored with a leading `/` and must not carry
a `**/` prefix, since `.omni` exists only at the workspace root.

REQ-16 A file with duplicated, crossed, or otherwise malformed fences must be a hard
error, and the run must write nothing.

REQ-17 The system must parse and validate every configured file before writing any
of them, so a malformed fence in one file never leaves another half-patched.

REQ-18 The system must detect and preserve each file's existing line ending (LF or
CRLF) independently.

REQ-19 A missing target file must be created; writing ignore rules in a directory
without git present must be harmless.

### Command surface

REQ-20 `omni ignore sync` (default) must patch the block into every configured file
and report what changed.

REQ-21 `omni ignore sync --dry-run` must print the block that would be written and
write nothing.

REQ-22 `omni ignore sync --check` must exit non-zero when any configured file is
missing the block or carries an out-of-date one, and must write nothing.

REQ-23 `omni ignore clean` must remove the block entirely from every configured
file while leaving all surrounding lines intact.

REQ-24 MCP must expose `ignore_sync` (accepting `dry_run` and `check`) and
`ignore_clean`, mirroring the projection tool shape.

### Breaking changes

REQ-25 This change is purely additive; it introduces a new command and a new
optional config key and does not alter the behavior of any existing command or
config. There are no breaking changes.

## Migration / upgrade path

No action is required to keep existing setups working. To adopt the feature, a repo
runs `omni ignore sync` once and then deletes its hand-authored omni-related
`.gitignore` lines, letting the managed fence own them going forward. `ignore sync`
only manages content inside its own fence, so it will not remove pre-existing
unfenced lines automatically; that cleanup is a one-time manual step.

## Technical Details

- The generic block-patching engine (fence parser, renderer, per-file patch, and the
  `IgnoreContributor` contract) lives in a new subsystem-agnostic crate so a future
  contributor (generators, tools, cache) can plug in without touching the parser. The
  two concrete contributors live in a separate crate.
- The `projections` contributor is a read of the projection ledger and a projection
  of each link's `dest`; it needs no config resolution, network, or filesystem walk.
- The `internal` set is kept honest by two mechanisms rather than a colocated list:
  shared path segment constants used by both the code that writes a `.omni/` dir and
  the contributor that ignores it (kills rename drift), plus a coverage test that
  walks everything created under `.omni/` during representative work and asserts each
  entry is covered except the `lock.json` keep-list (catches new subdirs in CI).
- Writing to `.ignore` and `.omniignore`, unlike `.gitignore`, also hides those paths
  from omni's own discovery walker, since omni honors both during discovery. For
  projection dests and `.omni/` state this is the intended outcome, but it is a real
  coupling that `.gitignore`-only tools do not have.
- `ignore sync` sits outside the task graph and does not feed omni's content hashing,
  so there is no stale-cache correctness risk.
- The `internal` toggle can later grow from a bare bool into a bool-or-map for
  per-subsystem control without a breaking schema change, the same pattern
  `enable_experimental` uses.

## Out of scope

- Auto-running `ignore sync` at the tail of `projection sync` (deferred; an opt-in
  `ignore.auto` can be added later).
- A `--local` flag targeting `.git/info/exclude` (deferred; serves only the git
  audience).
- Per-subsystem `ignore.sources` toggles (deferred; kept coarse as `internal`).
- Contributors for generators, tools, or the cache (the seam is built; the
  contributors are not).
- Any rewriting or reordering of the user's own ignore rules outside the managed
  block.
