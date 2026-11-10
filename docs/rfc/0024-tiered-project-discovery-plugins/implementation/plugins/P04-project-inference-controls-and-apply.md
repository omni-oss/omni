# P04: Project `inference` controls, `apply`, `defaults`, restricted contributions

**Size:** XL (split) · **Depends on:** R04, R05, F03, F05, F07, P02

## Goal

Add the configuration surface and composition behavior through which workspace and
project authors control inferred projects.

## RFC references

- [§7.1 project modes, apply, defaults](../../007.00-project-modes-and-apply.md)
- [§7.2 layer order](../../007.01-layer-order-and-merge-semantics.md), [§7.3 conflicts](../../007.02-multiple-plugins-and-conflicts.md#73-multiple-plugins-and-conflicts)
- [§6.3 data boundary](../../006.03-data-boundary-and-literal-values.md#63-data-boundary), [§17.1](../../017.00-review-findings-and-prerequisites.md#171-inferred-collection-operations-versus-conflict-detection)
- Fixtures C01-C07, C09, C12-C15, C19, C22, C26

## Slices (separate PRs)

1. **`inference` on `ProjectConfiguration`:** `mode: extend|replace` (default extend),
   `plugins: [ids]` allowlist (invalid with `replace`; narrows only). Controls belong to
   the concrete discovered file and are **not inherited** from bases; `base: true` still
   excludes templates.
2. **Restricted plugin-contribution types** in `omni_configurations`: the allowed
   field subset and additive-only collection profile (plain additions and `append`;
   reject `prepend`/`replace`/positional `merge`), no host-managed fields, no `extends`
   paths, literal-only environment values (uses R03 literal operands).
3. **Standard `apply`:** workspace-selected files per activation, resolved relative to
   the declaring file; reuse extension-closure traversal and raw-layer replay (R04/R07);
   tasks patch only the originating activation's inferred tasks; unmatched patches
   silently skipped with inspection reason; activation-bound unqualified references
   (F03); full operator language allowed (standard validators).
4. **Standard `defaults`:** `tasks.cache.enabled`, `key.files`, `key.env` only
   (baseline `append` semantics, rooted paths); outputs and other fields rejected. Slots
   into the F05 tier order between plugin-emitted and the host fallback.
5. **Composition of activations:** per-activation first, then combined under F07's
   conflict rules, beneath explicit extension/concrete layers; reordered loading never
   picks a winner; dependency edges resolve after composition and replacement.

## Acceptance

- C-fixtures listed above pass as real tests with synthetic contributor layers (no
  runtime needed).
- Multiple activations' disjoint task patches; activation-local subtraction keeps
  another activation's values; later explicit replacement overrides globally.
- Applied references survive public alias changes and cache round trips.
- Rejected plugin `prepend`/`replace` even if later overridden.
- `inference.plugins` allowlist errors with `replace`; stale allowlist ids fail clearly.
