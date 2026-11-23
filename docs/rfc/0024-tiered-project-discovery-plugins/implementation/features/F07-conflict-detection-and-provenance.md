# F07: Composition conflict detection and provenance

**Type:** feature · **Size:** L · **Depends on:** R04, R07

## Goal

When several **contributors** (layers beyond a single explicit file chain) set the same
field, detect it deterministically, keep a conflict marker until an explicit operation
resolves it, and record **provenance** (which layer/file/path set each effective value).
Without plugins, exercise it with synthetic contributor layers in tests; also use
provenance to improve existing explicit-layer diagnostics.

## Why it matters

Standalone: "where did this value come from?" for explicit `extends` chains. For plugins:
two plugins emitting the same task or field must conflict deterministically, independent
of scheduling order.

## RFC references

- [§7.3](../../007.02-multiple-plugins-and-conflicts.md#73-multiple-plugins-and-conflicts)
- [§17.1](../../017.00-review-findings-and-prerequisites.md#171-inferred-collection-operations-versus-conflict-detection)
- C04, C12, C17

## Code starting points

- Layer types and replay from R04; raw traversal from R07
- `crates/omni_types` (provenance types)
- `crates/omni_context/src/context.rs`, `extracted_data_validator.rs`
- Diagnostics: `crates/omni_diagnostic`

## Work

1. Provenance type: layer id, file path (workspace-normalized, R02), field path.
2. Conflict model: same leaf/subtree set by more than one unordered contributor; ordered
   (explicit extension chain) stays last-wins as today.
3. Markers survive until an explicit operation resolves them; resolution only affects
   the intended contribution.
4. Order independence: shuffled contributor order yields identical results/diagnostics.
5. Surface provenance via existing inspection output where cheap (full UI is F09).

## Acceptance

- Reordered contributors give identical outcomes.
- Explicit masking at leaf and subtree boundaries; equal text with different
  interpretation modes are not equal.
- Diagnostics name both contributors and files.
- No behavior change for pure explicit chains except added provenance.

## Follow-up proposal: separate authored configuration from runtime metadata

For contributors implementing F07 or adding source information to another
configuration loader. The existing representation works, but redesign this
boundary before expanding field-level provenance. This is a proposal, not a
prerequisite to merge the [F05.2 file-provenance slice](F05.2-contradiction-detector.md#file-provenance-f052-prerequisite).

### Problem

`CacheLayer.enabled_file` is loader data on an authored config type.
`#[serde(skip)]` hides it from the file format, but not from Rust equality,
mutation, or merges. `CacheLayer` needs a custom merge to keep `enabled` and its
file in sync. `ProjectConfiguration.file` and `.dir` have a similar boundary
problem: the loader supplies them, but their type also describes the public
config format. Adding one hidden field per tracked setting would repeat this
pattern and make it easy to retain a stale source after a replacement.

### Proposed boundary

| Stage | Owns |
| --- | --- |
| Authored config | User-writable values, presence and merge operators. These types define parsing, schema and authored serialization. |
| Loaded declaration | Authored data plus its loader-supplied source, such as a layer identifier, declaring file and path-resolution base. Sources without a file remain distinct from file-backed declarations. |
| Composition | Effective values and a provenance record produced by the **same** replay step. Record a winning source for scalar replacement and the surviving contributions for collections where diagnostics need them. |
| Resolved config | Effective config, runtime location and diagnostic provenance. Context, inspection and diagnostics read this information explicitly; they do not reconstruct it from serialized config. |

Build on `LoadedLayer` and ordered task declarations rather than introducing
metadata fields in `CacheLayer` or every other authored type. Associate an
origin with each loaded declaration once; have replay carry its origin with
the operation. If a provenance index is needed, make it an output of replay,
not a separately merged map. Use structured project/task/field components,
not delimiter-joined strings. Preserve the declaration's base directory
independently of the effective project root; inherited relative paths and
virtual projects need different location semantics.

Keeping hidden fields is cheap for one setting but scales poorly. Building
a separate index after composition avoids hidden fields but cannot recover
which declarations survived `replace` or combined through inheritance.
Recording provenance during replay avoids both problems. F07 should settle
the precise origin representation and which collection contributions to
retain before adding a shared API.

For example, an explicit `cache.enabled: true` in `app/project.omni.yaml`
replaces the inherited declaration even if the inherited value is already
`true`: the effective value stays `true`, and the winning file becomes the app
file. An omitted setting does not replace the inherited origin. A later
replacement from a source without a file must not report the old file.

This applies to other config loaders when they need runtime origin or location
data, such as workspace/worktree paths or projection and remote-source
declaration origins. Start with the first real caller in each subsystem; do
not require a shared wrapper or migrate all loaders at once. Authored fields
such as user-defined `meta` and configured paths are still authored data.

### Migration and checks

1. Move cache enablement provenance into loaded declarations and derive it
   during replay. Keep the existing diagnostic accessor and restore ordinary,
   value-only `CacheLayer` merging. Test omitted, equal-value and file-less
   replacements, task and project inheritance, and names containing `#`.
2. Move project file and directory into the resolved project boundary as a
   separate slice. Audit callers that read, hash or display locations. Test
   relative paths, extension closure order and virtual roots without a physical
   file before moving callers in other subsystems.
3. For each migration, check authored schema and serialization parity, unchanged
   effective values and cache keys, and no implicit provenance output. Revert
   each slice independently if those checks fail. Add new diagnostic output
   through explicit inspection types, not by serializing the loaded config.
