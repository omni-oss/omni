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
