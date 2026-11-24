# F07 implementation plan: composition provenance and conflicts

**Status:** approved (#164). F07.1 implemented and independently reviewed (PASS) on `feat/f07-provenance-redesign`; F07.2 implemented there, awaiting independent review; F07.3 onward not started. **Size:** Large.
**Risk:** High: composition and location changes can affect task behavior and
cache keys. The chosen direction is to finish the metadata redesign before
building conflict detection, rather than ship a parallel provenance walk.

## Scope and finish line

Complete [F07](F07-conflict-detection-and-provenance.md) in two phases:
**metadata and provenance** (F07.1–F07.2), then **conflicts and inspection**
(F07.3–F07.5). The slices below are review units, not a commitment to exactly
two PRs. Each must leave a working system.

- Explicit `extends` continues to work as before, including which files and
  directories are used for path resolution and cache inputs. Authored schemas,
  serialized configurations and existing explicit-only cache keys stay unchanged.
- Authored config types hold no loader-supplied file or directory fields.
  Resolved projects expose their location separately from authored values.
- Synthetic unordered contributors compose deterministically. Conflicts that
  survive explicit replay produce actionable diagnostics. Selected provenance
  is available through the existing inspection surfaces.
- No plugin host, activation runtime, new authored keys, cache-key version
  changes or virtual-project discovery. Fileless projects are exercised as
  resolved values in tests, not discovered by F07.

### Worked example

`apps/web/project.omni.yaml` extends `presets/web.omni.yaml`. The preset sets
`cache.enabled: true` and a relative cache glob; the app repeats `true`.
The effective setting points to the app file, the glob still resolves from
the preset directory, and the runnable project root stays `apps/web`. If the
app omits enablement, the preset remains the winner. If a later fileless
declaration replaces enablement, it wins without reporting the preset's file.

At the same root, synthetic contributors `frontend` and `native` supply
different descriptions and disjoint tasks. Reversing their order must produce
the same conflict. An explicit `description: web` resolves it; an unrelated
task override does not. The resulting inspection identifies the winning
explicit declaration without exposing unrelated configuration values.

## F07.1 — Replay values and provenance together

**Starting points:** `omni_configurations/src/layer.rs` (`LoadedLayer`,
`TaskDeclarations`, `finalize_composed`), `task_extension.rs`,
`omni_context/src/project_config_loader.rs` (`LoadedProjects::raw_layers`).
The extension graph already exposes raw closure order for explicit files.
Keep that order, including diamond deduplication and cycle errors, and use it
to drive composition. Do not change the merge contract for other graph users.

Keep declaration metadata in `omni_configurations` beside the replay logic.
Use a typed `DeclarationSource` rather than inferring the source kind from
an optional file path. Keep the relative-path base on `Origin` because both
source kinds can supply one.
Use structured identifiers; never key provenance by joined `project#task`
strings. Carry origin with task declarations through task extension and
short/long-form transitions. The operation that updates a tracked value
updates its origin, rather than calling `record_tracked` alongside a separate
merge. Start with `cache.enabled`, where the existing consumers can verify
the result.

```rust
// Illustrative: the base is a path-resolution input, not source identity.
struct Origin {
    source: DeclarationSource,
    base_dir: OmniPath,
}

enum DeclarationSource {
    File { path: Arc<OmniPath> },
    Fileless { id: LayerId },
}

enum CacheScope {
    Project,
    Task(TaskName), // local to this project's replay
}

struct Declared<T> {
    value: T,
    origin: Arc<Origin>,
}

fn replace_if_present<T>(
    slot: &mut Option<Declared<T>>,
    incoming: Option<T>,
    origin: &Arc<Origin>,
) {
    if let Some(value) = incoming {
        *slot = Some(Declared { value, origin: Arc::clone(origin) });
    }
}
```

For the tracked field, the winning value and origin travel together through
project and task replay; the non-tracked cache fields keep their ordinary
merge logic. Resolve task > project > host fallback from those results and
emit effective cache settings and provenance together. A host fallback is
not a declaration and has no `Origin`. A fileless winner has an `Origin`
but does not reuse a lower layer's file. Remove
`CacheLayer.enabled_file`, its special merge, and the stamp in
`ProjectLayer::load`. Keep `LoadedContext::cache_enabled_file` and current
F05.2/F09 output stable. **Keep `ProjectConfiguration.file` and `.dir`
unchanged in this slice.**

**Gate:** prototype the replay beside the current implementation; compare
effective values before switching. Cover omission, equal-value replacement,
fileless replacement, project/task inheritance, shorthand transitions, task
and project `extends`, diamonds and names with `#`. Pin schema and serialized
output, representative task hashes, and both files in the contradiction
diagnostic. Drop the old path only after parity. Revert the slice to restore
the existing merge and accessor implementation.

## F07.2 — Move project location out of `ProjectConfiguration`

**Depends on F07.1.** `ProjectConfiguration.file` and `.dir` are skipped by
Serde, but runtime code reads them. Move them into the resolved-project
boundary and migrate the callers in the same slice. Keep three concepts
separate: a declaration's relative-path base, the resolved project's root
directory, and the project's config location (file or virtual key).

```rust
// Illustrative ownership; reuse the existing location model at the context
// boundary, rather than adding a second file/virtual identity.
struct ResolvedProject {
    config: ProjectConfiguration, // no file or dir fields
    root_dir: OmniPath,
    location: ConfigLocation,    // File(...) or Virtual(LogicalProjectKey)
    provenance: ProjectProvenance,
}
```

| Current caller | Migration |
| --- | --- |
| `finalize` / `finalize_composed` in `layer.rs` | Return effective values separately from the concrete layer's location. Construct the resolved project in the loader; keep declaration bases on their origins. |
| `omni_context/src/project_config_entry.rs` | Reuse its `ConfigLocation`; classify discovery by the concrete file, not by a path inferred from the name. Preserve existing base and extends-only behavior. |
| `omni_context/src/project_data_extractor.rs` | Read runtime paths from the resolved project. Feed the same concrete file and extension-chain inputs to cache hashing in the same order, including existing repetitions. A virtual entry has no file input; do not manufacture one. |
| `omni_api/src/operations/project.rs` and `omni_mcp_core/src/tools/project.rs` | Take location from the resolved entry when selecting by path or displaying a directory. Keep existing wire fields and values; do not serialize the resolved wrapper as config. |

Audit all constructors and field readers, including tests, before deleting
the fields. This is an internal representation change with **no config-file
migration**, but downstream Rust callers using the public fields or struct
literals need an explicit migration note.

**Implemented (F07.2).** `ProjectConfiguration.file` and `.dir` are removed.
`FinalizedProject` carries the concrete layer's `file` and `dir` beside the
authored `config`; `ProjectConfigEntry` carries `root_dir` and `location`
(`from_finalized`, and `new_virtual(config, root_dir, key)`). The API
`ProjectConfigResponse` gains non-serialized `dir` and `file` so the MCP
`project_config` tool and CLI selection keep their values; the wire shape is
unchanged. Cache-key file inputs still come from the concrete file and its
extension chain in the same order.

**Migration note for Rust callers (no config-file migration).**
`ProjectConfiguration { file, dir, .. }` literals lose those two fields.
Read them from `FinalizedProject::{file, dir}` or `ProjectConfigEntry::{location,
root_dir}` instead. `ProjectLayer::finalize` no longer takes `file`/`dir`.
`ProjectConfigEntry::new_virtual` takes the root directory; `from_file` is
replaced by `from_finalized`.

**Gate:** pin the authored schema and API/MCP JSON shape, file/name selection,
relative paths from inherited declarations, closure order and diamond behavior,
and old/new cache input and digest parity for file-backed projects. Test a
virtual entry with a root but no file, without expanding discovery. Revert the
slice to restore the Rust fields without reverting F07.1.

## F07.3 — Compose unordered contributors without guessing a winner

**Depends on F07.1–F07.2.** Keep explicit traversal and unordered composition
separate: the extension graph provides ordered authored declarations only.
Test the new composition entry with synthetic, already validated contributions
identified by stable contributor IDs. Treat activation-local application as
finished input: one contributor's subtraction never edits another's values
([C04](../../007.04-composition-golden-fixtures.md#c04--application-subtraction-is-activation-local)).
Add a typed contributor variant to `DeclarationSource` here, rather than
encoding contributor identity as a file path or an anonymous fileless ID.
P-series work will wire its own validation and activation boundaries.

- Disjoint fields combine; semantically equal scalar/task fields coalesce and
  retain their contributors. Different values conflict, including equal text
  with different interpretation modes. Stable contributor order controls
  presentation and additive lists, not scalar winners.
- Keep conflicts until explicit replay overwrites that leaf or replaces an
  ancestor subtree. An unrelated edit or a merge that does not touch the leaf
  cannot clear it. Ordered explicit-only chains keep last-wins semantics.
- **Proposed collection rule:** compare dictionary contributions at the key
  or nested-field boundary rather than conflicting on whole dictionaries.
  Additive lists are ordered by contributor ID, not compared positionally for
  conflicts; preserve contributions until explicit operations finish. Follow
  [§7.3](../../007.02-multiple-plugins-and-conflicts.md#73-multiple-plugins-and-conflicts)
  for field-specific normalization. Do not add blanket deduplication or change
  explicit-only hashing in this slice. If a normalization correction would
  change existing keys, stop and scope it separately.
- Expose unresolved conflicts from composition as structured data: project
  root, field path and contributing origins. Invalid input is a validation
  error, never a conflict that a later override can hide.

**Gate:** shuffle contributor order and compare outcomes; cover C04,
[C12](../../007.04-composition-golden-fixtures.md#c12--cross-plugin-conflicts-require-a-relevant-explicit-resolution)
and [C17](../../007.04-composition-golden-fixtures.md#c17--normalize-duplicates-only-after-positional-operations).
Also cover equal values with distinct origins, interpretation modes, leaf and
subtree masks, and unrelated overrides. Revert this entry point without undoing
explicit-config provenance.

## F07.4 — Diagnose unresolved conflicts

**Depends on F07.3.** Convert unresolved conflicts into `omni_diagnostic`
errors at the composition boundary. Report the root, structured field path,
both contributor IDs and declaring files when known. Describe how an explicit
override resolves the field. Avoid raw configuration values in messages.
Do not change the existing duplicate-final-project-name check.

**Gate:** pin diagnostic code, stable order and CLI/MCP error detail. Include a
fileless contributor and a conflict surviving an unrelated explicit override.
Revert the new diagnostic mapping without changing replay semantics.

## F07.5 — Expose winning provenance through inspection

**Depends on F07.1–F07.4 and F09.** Start with the tracked `cache.enabled`
origin in `omni cache eligibility` and `cache_eligibility` MCP. Add an explicit
output field rather than serializing internal provenance. Text reports the
deciding tier and safe, workspace-relative file where available; JSON and MCP
share the same fields. Fileless and fallback decisions have no file, and no
raw values or file contents are returned. Keep F09 inspection non-executing.
Do not claim comprehensive field provenance before those fields are tracked.

**Gate:** snapshot text, JSON and MCP for inherited, overridden, fallback and
fileless decisions. Verify the inspection still runs no commands or environment
substitutions. Revert the output addition independently.

## Review and sequencing

Approve the plan before changing code. Confirm the proposed collection policy
before F07.3; it does not block the replay prototype. Keep F06.3 cache-key
changes and F03 alias plumbing out of these diffs; use the existing dependency
resolution contract, with plugin integration and new aliases wired separately.
For each slice, run scoped formatting, clippy and tests per `CONTRIBUTING.md`;
include config, loader, cache, API and MCP callers where affected. Require
independent review of each slice before handoff, and repeat explicit-only
parity checks after integration. Record the approved slice and baseline
fixtures so another implementer can resume from the same checks.
