# Requirements: Ledger reconciliation of removed projection sources (+ `unlink` backup handling)

- **RFC:** [0004-ledger-source-reconciliation](../rfc/0004-ledger-source-reconciliation.md)
- **Status:** Draft
- **Date:** 2026-08-29

## Summary

When a projection source is removed from configuration, `omni projection sync` already deletes its cached checkout and pin, but it leaves the source's projected links in the working tree and in the link ledger. For git sources those leftover links become broken, because their target checkout is gone. This change makes a full `sync` remove the leftover links of any source no longer in config, and it never deletes a file the user has edited. It also replaces the on/off backup switch on `omni projection unlink` with a three-way choice that can now restore the file a projection originally displaced. This ships as a fix. It closes a missed requirement in the projections feature released in v0.1.0, not a new feature.

## Goals

- After a full `omni projection sync`, the working tree and the link ledger contain only the sources currently declared in configuration.
- Removing a source from config never leaves a broken link behind.
- Cleanup never destroys a file the user has edited.
- `omni projection unlink` offers an explicit, non-destructive choice for the backups a projection created. Leave them, delete them, or restore them.
- The `--clean-backups` flag keeps its current meaning, so nobody's existing scripts break.

## Non-goals

- Restoring backups during a `sync`. The restore choice is available only through the manual `unlink` command. Automatic reconciliation always leaves backups in place.
- Changing what `omni projection prune` does. It still removes only ledger links whose destination is a dangling symlink.
- Changing how caches and pins for removed sources are collected.
- Triggering a projection sync from any other command. Reconciliation runs only when the user runs `omni projection sync`, or the equivalent API/MCP call.

## Requirements

### Behavior — automatic reconciliation

REQ-01 On a full, unfiltered, non-dry-run `omni projection sync`, the system must remove every projected destination the ledger records for a source that is no longer in the current configuration.

REQ-02 On the same run, the system must drop those sources from the ledger, so the ledger afterward lists only currently configured sources.

REQ-03 The system must remove a copy-type link whose destination still matches its source, and must keep a copy-type link whose destination was edited since projection, warning and naming each kept file.

REQ-04 Reconciliation must never touch a file the ledger does not record as an omni-owned link.

REQ-05 Removing a source must not leave a dangling link in the working tree. The system must tear down a source's links in the same run that removes its cached checkout, or not at all.

REQ-06 When the ledger is missing or unreadable, the system must treat it as empty and remove nothing.

REQ-07 A `sync` scoped to a single source must affect only that source. Both the ledger cleanup and the cached-checkout cleanup are skipped on a scoped run.

REQ-08 The `sync` result must list the destinations reconciliation removed, and must include the kept-modified-copy warnings from REQ-03.

REQ-09 On a full `--dry-run` sync, the system must report the destinations it would remove, and the copies it would keep as warnings, without changing anything on disk or in the ledger.

REQ-10 On a scoped `--dry-run` sync, the system must report no orphan removals, matching the corresponding real scoped run.

### Behavior — `unlink` backup handling

REQ-11 `omni projection unlink` must support three backup outcomes. Leave the recorded backup in place (default), delete it, or restore it to the link's original destination.

REQ-12 When the caller gives no backup option, `unlink` must leave any recorded backups untouched.

REQ-13 When the caller requests restore, the system must move the recorded backup back to the link's original destination only after it has removed the link.

REQ-14 Restore must never overwrite existing content. If the destination is still occupied, for example a kept edited copy, or the recorded backup is gone, the system must skip that restore and warn.

REQ-15 A skipped or failing restore must not abort the rest of the `unlink` operation.

REQ-16 The `unlink` result must list which backups it restored, alongside the destinations it removed and any warnings.

### Configuration and user-facing surface

REQ-17 The CLI must offer two `unlink` flags, one to delete backups and one to restore them, and must reject an invocation that passes both.

REQ-18 The `--clean-backups` flag must keep its exact name and its current "delete the backups" meaning.

REQ-19 The API and MCP request for `unlink` must express backup handling as one optional three-value choice (leave, clean, restore), where absence means leave.

## Breaking changes

REQ-20 In v0.1.0 a source-scoped `sync` still deleted the cached checkouts of sources removed from config. After this change, a scoped `sync` touches only the named source, and removed-source cleanup waits for the next full `sync`. Affects: anyone relying on a scoped `sync` to clean up other sources. None is known, since projections shipped in v0.1.0 days before this change.

REQ-21 In v0.1.0 the `unlink` request carried a boolean `clean_backups` field. After this change, it carries an optional three-value backup-handling choice. Affects: API and MCP callers that send the boolean. The old boolean is accepted for one release as a deprecated alias meaning "clean", then removed.

## Migration / upgrade path

CLI users do nothing. `--clean-backups` behaves as before, and `--restore-backups` is new and optional.

Config authors do nothing. There is no config schema change. The first full `omni projection sync` after upgrading cleans up the links of any already-removed sources. If a user removed a git source before upgrading and is left with broken links, a full `sync` resolves them, and so does `omni projection prune`.

API and MCP callers that send `clean_backups: true` keep working for one release through the deprecated alias, but should move to the new backup-handling field before it is removed.

## Technical Details

- Reconciliation reuses the same non-destructive teardown path as `unlink`: the edited-copy safety check, link removal, and empty-parent-directory cleanup. Its safety behavior is therefore identical to `unlink` by construction.
- One "is this a full sync" condition guards both the ledger cleanup and the cached-checkout cleanup, and the two run next to each other. They stay in lockstep, so a source's links can never outlive its checkout.
- The system persists the ledger before it runs the checkout cleanup, so an interrupted sync converges to the correct state on the next run.
- The three-way backup handling is one shared model behind the CLI, API, MCP, and the teardown routine. Only the CLI splits it into two mutually exclusive flags.
- The ledger's on-disk format does not change. Only its contents do.

## Out of scope

- Restoring backups automatically during `sync`.
- Changing `prune` behavior.
- Changing cache and pin collection for removed sources.
- Any flag or config to disable automatic reconciliation. It is on by default with no toggle.
