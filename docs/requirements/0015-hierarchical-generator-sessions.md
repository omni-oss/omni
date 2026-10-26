# Requirements: Hierarchical generator sessions

- **RFC:** [0015-hierarchical-generator-sessions](../rfc/0015-hierarchical-generator-sessions.md)
- **Status:** Implemented
- **Date:** 2026-09-25 (revised 2026-09-26)

## Summary

Generator session files (`.omni/generator.json`) currently remember inputs and
target overrides per directory only. This change makes session restoration
hierarchical: a run merges every session file from the output directory up to
the repo root, with deeper files winning over shallower ancestors. Saving stays
anchored to the output directory and writes only the values specific to that
directory, so shared defaults placed higher in the tree are inherited but never
copied down or rewritten.

The session file also gains a **shared** block that provides input/target
defaults to every generator run beneath it (for example a JS package scope,
license, or author that is not owned by any single generator). A
generator-specific value overrides the shared value; the nearer file wins. All of
this lives inside a redesigned, versioned envelope
(`{ version, root, generators, shared }`) that keeps generator names in their own
`generators` namespace, so no generator name — including `root` — can collide
with a reserved key. The on-disk format is redesigned and breaking; because omni
is pre-release, legacy session files are not migrated — they lack the `version`
tag and fail to load with an error rather than being read or silently dropped.

## Goals

- Let a session file placed higher in the tree provide input and target defaults
  for everything generated beneath it.
- Let a deeper file, or the output directory's own file, override those defaults
  for its subtree.
- Keep explicit command-line inputs and targets winning over any inherited
  session value.
- Treat the repo-root session as a first-class, hand-editable defaults file
  (for example JS package scope, license, author) that every run inherits.
- Provide a shared block whose values apply to every generator in the subtree,
  overridable by a generator-specific value and by any nearer file.
- Give users a way to seal off inheritance for a subtree before the repo root.
- Give users a way to turn the whole behavior off and get today's single-file
  behavior back.
- Never rewrite ancestor session files during a run.
- Store sessions in a versioned format that keeps generator names collision-free,
  so any name (including `root`) is valid and future format changes are
  migratable.

## Non-goals

- No new configuration on `workspace.omni.yaml` or `project.omni.yaml`; this
  concerns only the existing session file.
- No hierarchical merge for anything other than generator sessions (environment
  variables, projects, and tasks are out of scope).
- No change to how a generator's own declared inputs, targets, or defaults work.
- No command-line or prompt surface for writing shared values; the shared block
  is hand-authored (or seeded once at workspace generation).
- No migration of legacy session files. The format carries an explicit `version`
  tag from the start, but legacy (unversioned) files are not migrated — they fail
  to load with an error.

## Requirements

### Behavior

REQ-01 When a generator runs, the system must collect every
`.omni/generator.json` from the output directory upward and merge them into one
effective session used to seed the run.

REQ-02 In the merge, a value from a deeper file (closer to the output directory)
must override the same key from a shallower ancestor.

REQ-03 The upward walk must stop at the workspace (repo) root and must never
read a session file outside the workspace.

REQ-04 Explicit command-line input and target values must win over any value
restored from a session at any level.

REQ-05 The effective precedence, highest first, must be: command-line override;
then, walking from the output directory upward, each file's generator-specific
value followed by that file's shared value; up to the topmost consulted file;
then the generator's own defaults.

REQ-06 A save must write only the values specific to the output directory and
must never persist the merged or inherited values.

REQ-07 A save must write only to `output_dir/.omni/generator.json` and must
never create or modify any ancestor session file.

REQ-08 A run in which every value is inherited, and where the user provided
nothing explicitly, must write no output-directory session file.

REQ-09 Overriding a single inherited value at the output directory must persist
only that one value.

REQ-10 A value the user passes explicitly on the command line must be persisted
to the output-directory session even when it equals the inherited value.

REQ-11 When a previously saved override is reverted to the inherited value
through a non-explicit path (for example an interactive prompt), the system must
remove that override from the output-directory file, pruning the file entirely
if it becomes empty.

REQ-12 The system must not delete an output-directory session file whose only
remaining content is a `root: true` marker or a `shared` block; it must retain
that file and rewrite only the per-generator entries.

REQ-13 A dry run must not write, prune, or delete any session file.

### Configuration

REQ-14 A session file may declare a top-level `root: true` marker that stops the
upward walk at that file, inclusive, so no file above it is consulted.

REQ-15 The `root` marker must be user-editable and must survive a delta save
(the system re-emits it when persisting or pruning the file's generators).

REQ-16 Generator names are unrestricted: any string, including `root`, `version`,
and `shared`, is a valid generator name, because generator entries are stored in
their own `generators` namespace and never share the top level with reserved
keys.

REQ-17 The system must provide an `inherit_session` option, enabled by default,
that when disabled reverts a run to reading and writing only the output
directory's own session file.

REQ-18 On the command line the option must accept `--inherit-session` (enable)
and `--inherit-session=false` (disable), matching the shape of `--save-session`
and `--ignore-session`; there is no `--no-inherit-session` spelling.

REQ-19 `inherit_session` and `ignore_session` must compose: with
`ignore_session` set, no session is read at all and `inherit_session` has no
effect; with `ignore_session` unset, `inherit_session` selects walk-up merge
(default) or single-file read.

### Shared session data

REQ-24 A session file may declare a `shared` block of input and target values
that seed every generator run governed by that file, in addition to the
generator-specific entries.

REQ-25 Within a single file, a generator-specific value must override the shared
value for the same key.

REQ-26 Across the tree, the nearer file must win: a shared value in a file closer
to the output directory must override a generator-specific value in a farther
ancestor (composition is depth-major, with specificity as the intra-file
tiebreaker).

REQ-27 A run must never write to or modify the `shared` block; it may rewrite
only the per-generator entries. The shared block (and the `root` marker) must be
preserved verbatim across every save and prune.

REQ-28 The inherited baseline used to compute the saved delta must include the
output-directory file's own `shared` block, so a run value equal to a shared
value is never copied down into a generator-specific entry.

### Format

REQ-29 A session file must be stored as a versioned envelope with a top-level
`version` tag, a `root` marker, a `generators` map, and a `shared` block.
Generator entries live under `generators`, so no generator name can collide with
a reserved key. A file that carries only a `root: true` marker, only a `shared`
block, or both (with no generator entries) must be valid and round-trip
unchanged.

### Compatibility

REQ-20 (superseded by REQ-30) Legacy session files no longer parse as before;
the format is redesigned and breaking.

REQ-21 The `version` tag must always be written; the `root` key must be written
to disk only when it is `true`; and empty `generators` and `shared` sections must
be omitted, so a minimal delta stays small (an empty session is just its
`version`).

REQ-22 Re-running an unchanged leaf must be idempotent: it writes no file and
does not touch any ancestor file.

## Breaking changes

REQ-23 Previously, a workspace-level `.omni/generator.json` had no effect on runs
in subdirectories. After this change, when inheritance is enabled (the default),
a workspace-root or any ancestor session file becomes a shared default for runs
beneath it, including `--project` runs where the output directory is the project
directory. Affects: any repo that already has session files at more than one
level in the tree.

REQ-30 The on-disk session format is redesigned (the versioned envelope in
REQ-29) and is not backward compatible. Legacy session files — both the original
bare generator map and the earlier `{ root, <generator>… }` shape — lack the
`version` tag, so they fail to deserialize with an error that propagates to the
caller. They must not be read or silently dropped; the user removes or recreates
them. Note that a single legacy file anywhere in the consulted ancestry aborts
runs beneath it until removed. No automated migration is provided. Affects: any
repo with existing `.omni/generator.json` files. Because omni is pre-release,
this break is accepted.

## Migration / upgrade path

Existing inheritance behavior needs no action. For the format break (REQ-30),
no migration is required or provided: a legacy (unversioned) file fails to load
with an error, so the user removes or recreates it; the next run seeds a fresh
versioned file. Users who want to preserve specific values can re-enter them once
(or move them into a `shared` block). Users who do not want inheritance can
disable it per run with `--inherit-session=false`. Users who want a subtree to
ignore ancestor defaults can add `root: true` to a session file in that subtree.
Repos that already have session files at multiple levels should be aware that
ancestor values now flow into descendant runs by default (see REQ-23).

## Technical Details

- The design keeps two sessions at the API layer: an `effective` session (the
  full walk-up merge, used only to seed the run) and an `outdir` session (the
  output-directory-specific delta, the only thing written).
- Each file is collapsed to a per-generator view (`shared` then the
  generator-specific entry overriding it) before the shallow-to-deep fold, which
  is what makes composition depth-major with an intra-file specificity tiebreak.
- The delta is computed by subtracting an inherited baseline from the effective
  run result, per generator and per key. The baseline is the same fold, except
  the output-directory file contributes only its `shared` block (not its own
  per-generator entry), so a value equal to the file's own shared value is not
  re-persisted.
- Delta equality uses plain value comparison with no normalization or filesystem
  path canonicalization. An inherited-unchanged value is a clone of the baseline
  value and compares equal, so it is dropped; a differing value is kept.
- Keys the caller provided explicitly this run are exempt from the subtraction
  and always persist, even when equal to the baseline. An interactive answer
  that merely re-types the inherited value is indistinguishable from accepting
  it and still collapses; full provenance tracking for that case is deferred.
- The save gate diffs the computed delta against the on-disk output-directory
  file rather than checking whether the delta is empty, which gives idempotency
  and avoids leaving a stale override on disk after a revert.
- The on-disk format is the redesigned envelope
  `{ version, root, generators, shared }`, an internally-tagged versioned
  structure mirroring the lockfile's `version`-tagged enum. Carrying the version
  from the start makes future format changes migratable; a legacy file with no
  `version` fails to deserialize with a natural error (no silent drop).

## Out of scope

- New configuration surface on `workspace.omni.yaml` or `project.omni.yaml`.
- Hierarchical merge for environment variables, projects, or tasks.
- Changes to a generator's own declared inputs, targets, or defaults.
- A command-line or prompt surface for writing shared values (the shared block
  is hand-authored or seeded at workspace generation).
- Provenance-tracked collection that would let interactive answers equal to an
  inherited value also persist (deferred).
