# F07 implementation plan: composition provenance and ordered precedence

**Status:** approved (#164). F07.1 and F07.2 are merged ([#172](https://github.com/omni-oss/omni/pull/172); final review PASS per the PR); F07.3 is merged ([#194](https://github.com/omni-oss/omni/pull/194); see [its plan](F07.3-ordered-composition-plan.md)); F07.4 (override notice) is built in review; F07.5 is not started. **Size:** Large.
**Risk:** High: composition and location changes can affect task behavior and
cache keys. The chosen direction is to finish the metadata redesign before
building ordered composition, rather than ship a parallel provenance walk.

## Scope and finish line

Complete [F07](F07-conflict-detection-and-provenance.md) in two phases:
**metadata and provenance** (F07.1–F07.2), then **ordered composition and inspection**
(F07.3–F07.5). The slices below are review units, not a commitment to exactly
two PRs. Each must leave a working system.

- Explicit `extends` continues to work as before, including which files and
  directories are used for path resolution and cache inputs. Authored schemas,
  serialized configurations and existing explicit-only cache keys stay unchanged.
- Authored config types hold no loader-supplied file or directory fields.
  Resolved projects expose their location separately from authored values.
- Synthetic ordered contributors compose deterministically: the later one wins,
  explicit replay still overrides all of them, and overridden contributors are
  reported. Selected provenance is available through the existing inspection surfaces.
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

At the same root, synthetic contributors `frontend` and `native`, in that order,
supply different descriptions and disjoint tasks. `native` wins the description and
`frontend` is reported as overridden. Reversing the order flips the winner;
reversing only scheduling does not. An explicit `description: web` overrides both;
an unrelated task override does not. The resulting inspection identifies the winning
declaration and the overridden contributors without exposing configuration values.

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

| Current caller                                                                | Migration                                                                                                                                                                                                                           |
| ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `finalize` / `finalize_composed` in `layer.rs`                                | Return effective values separately from the concrete layer's location. Construct the resolved project in the loader; keep declaration bases on their origins.                                                                       |
| `omni_context/src/project_config_entry.rs`                                    | Reuse its `ConfigLocation`; classify discovery by the concrete file, not by a path inferred from the name. Preserve existing base and extends-only behavior.                                                                        |
| `omni_context/src/project_data_extractor.rs`                                  | Read runtime paths from the resolved project. Feed the same concrete file and extension-chain inputs to cache hashing in the same order, including existing repetitions. A virtual entry has no file input; do not manufacture one. |
| `omni_api/src/operations/project.rs` and `omni_mcp_core/src/tools/project.rs` | Take location from the resolved entry when selecting by path or displaying a directory. Keep existing wire fields and values; do not serialize the resolved wrapper as config.                                                      |

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
extension chain: the same entries, repeats included, with the project's own
file last. The order of the extension closure itself was already hash-random
before F07 and still is, so parity fixtures pin the sorted entries.

**Migration note for Rust callers (no config-file migration).**
`ProjectConfiguration { file, dir, .. }` literals lose those two fields.
Read them from `FinalizedProject::{file, dir}` or `ProjectConfigEntry::{location,
root_dir}` instead. `ProjectLayer::finalize` no longer takes `file`/`dir`.
`ProjectConfigEntry::new_virtual` takes the root directory; `from_file` is
replaced by `from_finalized`. `FinalizedProject::file` is now an
`Option<OmniPath>` (`None` for fileless layers). `omni_api::ProjectConfigResponse`
gained public fields `dir` and `file`, so struct literals and destructuring
without `..` break. `LoadedLayer`'s fields are private: read the layer with
`LoadedLayer::layer()`, replace it with `with_layer(..)`, and edit it with
`update_layer(|layer| ..)`, which drops the tracked `cache.enabled` so it is
re-derived on the next composition.

**Gate:** pin the authored schema and API/MCP JSON shape, file/name selection,
relative paths from inherited declarations, closure order and diamond behavior,
and old/new cache input and digest parity for file-backed projects. Test a
virtual entry with a root but no file, without expanding discovery. Revert the
slice to restore the Rust fields without reverting F07.1.

## F07.3 — Compose ordered contributors; the later one wins

**Depends on F07.1–F07.2.** Keep explicit traversal and contributor composition
separate: the extension graph provides ordered authored declarations only.
Test the new composition entry with synthetic, already validated contributions
given as an ordered sequence of stable contributor IDs, the same shape the
activation list will supply. Treat activation-local application as
finished input: one contributor's subtraction never edits another's values
([C04](../../007.04-composition-golden-fixtures.md#c04--application-subtraction-is-activation-local)).
Plugin-level `cache.enabled` needs care: `CacheEnabledSource` has one `Plugin` variant today
(`crates/omni_configurations/src/cache_enabled.rs`) and `CacheEnabled::resolve` keeps the
last of equal candidates, so resolve plugin task over plugin project before passing a
single `Plugin` candidate, or split the variant, so C01 holds.
Add a typed contributor variant to `DeclarationSource` here, rather than
encoding contributor identity as a file path or an anonymous fileless ID.
P-series work will wire its own validation and activation boundaries.

- Disjoint fields combine; semantically equal scalar/task fields coalesce and
  retain their contributors. Different values: the later contributor wins,
  including equal text with different interpretation modes. Compatible task
  declarations merge property by property, later winning; a different declaration
  kind replaces the earlier one as a whole. This applies only when contributors
  share a task key (the synthetic same-key contributors of these tests, explicit
  layers); plugin-inferred tasks stay activation-scoped and an ambiguous automatic
  alias stays an error ([C20](../../007.04-composition-golden-fixtures.md#c20--alias-and-inheritance-failures-validate-surviving-graphs)).
- Tiered fields (`cache.enabled`) resolve tiers first; order decides within a tier
  ([C01](../../007.04-composition-golden-fixtures.md#c01--cache-enablement-tier-matrix)).
- Plugin output never carries `cache.remote`; reject it at the output boundary
  (P-series wiring).
- No conflict markers. Explicit replay, as today, overwrites a leaf or replaces an
  ancestor subtree and always beats every contributor; explicit-only chains keep
  last-wins semantics. Scalar conflicts inside one contributor or between explicit
  layers replay normally.
- **Proposed collection rule:** compare dictionary contributions at the key
  or nested-field boundary rather than whole dictionaries. Additive lists
  concatenate in contributor order, not positionally compared (plugin lists use
  only the `append` envelope; a plain list or bare pattern from a plugin or in
  activation `defaults` is rejected at the output boundary, amended 2026-11-26 and
  2026-11-27); preserve
  contributions until explicit operations finish. Follow
  [§7.3](../../007.02-multiple-plugins-and-conflicts.md#73-multiple-plugins-and-conflicts)
  for field-specific normalization. Do not add blanket deduplication or change
  explicit-only hashing in this slice. If a normalization correction would
  change existing keys, stop and scope it separately.
- No ID-based ordering is needed: contributor order is the given sequence, and
  IDs only name contributors. Built-ins have no special position.
- Carry, for each winning leaf, the overridden contributors with differing
  values, as structured data (project root, field path, contributor origins), for
  F07.4 and F07.5. Invalid input is a validation error, never something a later
  contributor or override can hide.

**Gate:** same ordered sequence under shuffled scheduling gives identical
output; reversing the sequence flips the winner and the overridden set of a field that plugins
set to different values at the same tier with no explicit override (an explicit layer or
a higher `cache.enabled` tier keeps its winner). Cover C04,
[C12](../../007.04-composition-golden-fixtures.md#c12--later-activation-wins-explicit-configuration-overrides-both)
and [C17](../../007.04-composition-golden-fixtures.md#c17--normalize-duplicates-only-after-positional-operations).
Also cover equal values with distinct origins (no overridden entry), interpretation
modes, leaf and subtree masks by explicit layers, and unrelated overrides. Revert this entry point without undoing
explicit-config provenance.

**Implemented (F07.3).** `compose_ordered` in `omni_configurations` (`ordered.rs`,
`overrides.rs`); nothing in production calls it yet. Public additions, all new
names: `ContributorId`, `DeclarationSource::Contributor`, `Contribution`,
`Composed { layer, overrides }`, `ComposeError`, `EnabledDecl`, `EnabledScope`,
`CacheScope`, `resolve_plugin_tier`, `FieldOverride`, `FieldPath`, `OverrideTier`
and `FinalizedProject::task_cache_enabled_scope`. **Migration note for Rust
callers:** `FinalizedProject` gains a public field, so struct literals and
destructuring without `..` break; a `match` on `DeclarationSource` needs a
`Contributor` arm. No config-file migration; explicit-only finalized values,
cache keys and hashes are unchanged (pinned by `tests/explicit_only_parity.rs`
and the collector, context and executor goldens).
Differences from the plan: `FieldOverride` carries a `tier` (`Emitted` or
`Applied`) because applied and plugin-emitted `cache.enabled` values are compared
within their own tier; the guard error for a bad contributor form names the field
as diagnostic text (`ComposeError::ForbiddenForm`), not a provenance key.

## F07.4 — Notify when a later contributor overrides

**Depends on F07.3.** Keep this slice, now small, because an override between
plugins must not be silent. At the composition boundary raise one aggregated
info-level `omni_notice::Notice` (RFC 0023; declared like `cache.dependency_demoted` in
`crates/omni_task_executor/src/notices.rs`; illustrative code
`composition.plugin_override`) with counts of overridden fields and projects. Raise it
only when a later contributor's value differs from an earlier one and no explicit
layer replaces the field (provenance still lists the overridden contributors
then); coalesced values and additive lists raise nothing. Never
put raw values in the message. Details for `cache.enabled` overrides live in the F07.5 inspection field; details for
other fields, such as a description override, wait for P12 inspection, so the notice
carries counts only until then. Do not
change the existing duplicate-final-project-name check, which stays an error.

**Implemented (F07.4).** `omni_configurations::PluginOverride` (code `composition.plugin_override`, info, not essential, message `{fields} field(s) in {projects} project(s) were overridden by a later plugin`), `PluginOverride::from_overrides` (aggregates the records of any number of projects) and `Composed::override_notice`. It counts distinct `(root, field)` records that have an overridden contributor and are not `masked_by_explicit`. Nothing in production raises it yet, because nothing calls `compose_ordered`; the future caller passes the value to `notice!`. Counts only: no values, ids or paths. See the [F07.4 plan](F07.4-override-notice-plan.md).

**Gate:** pin the notice code, level and message text; cover differing, equal,
explicitly replaced and fileless-contributor cases. Revert the notice without
changing composition.

## F07.5 — Expose winning provenance through inspection

**Depends on F07.1–F07.4 and F09.** Start with the tracked `cache.enabled`
origin in `omni cache eligibility` and `cache_eligibility` MCP. Add an explicit
output field rather than serializing internal provenance. Text reports the
deciding tier and safe, workspace-relative file where available, and now also names
the overridden contributors (plugin ids and declaration locations, never values);
JSON and MCP share the same fields. Fileless and fallback decisions have no file, and no
raw values or file contents are returned. Keep F09 inspection non-executing.
Do not claim comprehensive field provenance before those fields are tracked:
F07.5 exposes `cache.enabled` overrides first, and other fields wait for P12
inspection.

**Locations (decided in the [F07.5 plan](F07.5-winning-provenance-plan.md)):** this slice records the declaration location of overridden `cache.enabled` contributors (`Composed::override_locations`, beside `FieldOverride`). Only applied-tier declarations have a file; a plugin-emitted or fileless one reports none, and other fields wait for P12.

**Gate:** snapshot text, JSON and MCP for inherited, overridden, fallback and
fileless decisions, and for a decision with overridden contributors. Verify the
inspection still runs no commands or environment substitutions. Revert the output
addition independently.

## Decision and audit

**Decision (2026-11-26, maintainer approved).** Activation-list order decides
precedence between plugins; the later listed wins. Reasons: opt-in activation makes
the list explicit user configuration; Nx uses the same rule; a hybrid (error by
default, order resolves) collapses because the list is always present. Built-ins have
no special order. Declaration layers go less specific first. The earlier model
(conflicts are errors, no order decides winners) is superseded; see
[§7.3](../../007.02-multiple-plugins-and-conflicts.md#73-multiple-plugins-and-conflicts)
and [§11](../../011.00-alternatives-and-discussion.md#11-alternatives-and-discussion-points).

**Audit appendix: places that stated or depended on the old rule.**

| Place                                                                                  | Action                                                                                    |
| -------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| 007.02 §7.3 (rule, ID order, list-restriction reason, final conflict paragraph)        | Rewritten; list restrictions kept, reason restated                                        |
| 007.01 §7.2 item 1                                                                     | Reworded: list order decides cross-activation precedence                                  |
| 007.00 (replace note, apply combination, accumulated value)                            | Reworded to ordered precedence                                                            |
| 007.03 (two sentences on conflict/last-writer rules)                                   | Reworded; duplicate project names stay errors                                             |
| 007.04 C04                                                                             | Order now activation-list; reversed-list case added                                       |
| 007.04 C12                                                                             | Rewritten as ordered-precedence fixture with explicit override                            |
| 007.04 C17, C14                                                                        | Unchanged: no dependence                                                                  |
| 005.00 (list order never chooses a winner; stable effective-id order; execution order) | Clarified: only id collisions; ordered by list                                            |
| 008.00, 008.01                                                                         | Reordering recomposes, fragments stay valid; composed-result keys include the ordered ids |
| 011.00                                                                                 | Alternative moved to chosen; old rule and hybrid recorded                                 |
| 000.00, 010.00, 013.00, 016.00                                                         | Wording only                                                                              |
| 017.00 §17.1                                                                           | Conflict markers replaced by ordered precedence; fixtures list updated                    |
| 018.00 §18.1                                                                           | Resolution note; no open decision                                                         |
| 006.00:223 "no last-writer-wins merge"                                                 | Left: stage state commits, not configuration composition                                  |
| 008.02 implicit/explicit cache-enablement conflict (dependency demotion)               | Left: dependency eligibility, not plugin precedence                                       |
| P10 conflicting lockfiles                                                              | Left: package-manager detection, unrelated                                                |
| 013.00:252 "manufacture conflict precedence"                                           | Reworded to "manufacture precedence"                                                      |
| 014.00 activation-id order for version negotiation                                     | Left: protocol selection, not composition                                                 |
| 004.01, 006.02, 006.04 conflicts                                                       | Left: conflicts within one context or one owner's claims, unrelated                       |
| F07 feature doc, F07.3-F07.5, sequencing                                               | Rewritten here                                                                            |
| tasks.yaml F07 and P04.5 titles, owner text; SCHEDULE.md                               | Edited; regenerated with `python3 schedule.py` and `oxfmt`                                |
| README, P04, P08, P16, R04                                                             | Wording only                                                                              |

**Decided by the maintainer on 2026-11-26** (also in
[§7.3](../../007.02-multiple-plugins-and-conflicts.md#73-multiple-plugins-and-conflicts)).
(1) Ambiguous automatic alias is the one place list order does not decide a winner;
it stays an error. (2) `cache.remote` is excluded from plugin output.

## Review and sequencing

Approve the plan before changing code. Confirm the proposed collection policy
before F07.3 (precedence itself is decided, above); it does not block the replay prototype. Keep F06.3 cache-key
changes and F03 alias plumbing out of these diffs; use the existing dependency
resolution contract, with plugin integration and new aliases wired separately.
For each slice, run scoped formatting, clippy and tests per `CONTRIBUTING.md`;
include config, loader, cache, API and MCP callers where affected. Require
independent review of each slice before handoff, and repeat explicit-only
parity checks after integration. Record the approved slice and baseline
fixtures so another implementer can resume from the same checks.
