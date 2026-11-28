# P04: Project `inference` controls, `apply`, `defaults`, restricted contributions

**Size:** XL (split) · **Depends on:** R04, R05, F03, F05, F07, P02

## Goal

Add the configuration surface and composition behavior through which workspace and
project authors control inferred projects.

## RFC references

- [§7.1 project modes, apply, defaults](../../007.00-project-modes-and-apply.md)
- [§7.2 layer order](../../007.01-layer-order-and-merge-semantics.md), [§7.3 multiple plugins](../../007.02-multiple-plugins-and-conflicts.md#73-multiple-plugins-and-conflicts)
- [§6.3 data boundary](../../006.03-data-boundary-and-literal-values.md#63-data-boundary), [§17.1](../../017.00-review-findings-and-prerequisites.md#171-inferred-collection-operations-versus-conflict-detection)
- Fixtures C01-C07, C09, C12-C15, C19, C22, C26

## Slices (separate PRs)

1. **`inference` on `ProjectConfiguration`:** `mode: extend|replace` (default extend),
   `plugins: [ids]` allowlist (invalid with `replace`; narrows only). Controls belong to
   the concrete discovered file and are **not inherited** from bases; `base: true` still
   excludes templates.
2. **Restricted plugin-contribution types** in `omni_configurations`: the allowed
   field subset and additive-only collection profile (`append` envelope only;
   reject plain lists and bare `key.files` patterns, `prepend`/`replace`/positional
   `merge`), shared with activation `defaults` (maintainer-confirmed 2026-11-27), no host-managed fields, no `extends`
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
   ordered-precedence rules (activation-list order, later wins, within a tier for `cache.enabled`), beneath explicit
   extension/concrete layers; reordered loading never picks a winner; dependency edges resolve after composition and replacement.

## Acceptance

- C-fixtures listed above pass as real tests with synthetic contributor layers (no
  runtime needed).
- Multiple activations' disjoint task patches; activation-local subtraction keeps
  another activation's values; later explicit replacement overrides globally.
- Applied references survive public alias changes and cache round trips.
- Rejected plugin plain list and `prepend`/`replace` even if later overridden, with a
  structured error at the output boundary; authored plain lists still replace.
- A finalized config holding a plugin-emitted root reference serializes and
  inspects cleanly (`ProjectConfigResponse`, `print-config`), using the internal
  serialization added in [#147](https://github.com/omni-oss/omni/pull/147).
- `inference.plugins` allowlist errors with `replace`; stale allowlist ids fail clearly.

## F03 plugin-scoped variants (carried from F03)

Not built in F03 because they need inferred declarations and activations. P04.3 owns:

- Activation-scoped tasks: two activations emitting a local `build` stay two distinct tasks and never merge; a plugin cannot emit aliases or touch another activation's tasks ([§7.3](../../007.02-multiple-plugins-and-conflicts.md#73-multiple-plugins-and-conflicts)).
- Plugin-qualified references (`plugin::project#task`) bind to the originating activation; remove the "plugin scopes are not supported" rejections in `task_dependency_configuration.rs` and `task_alias.rs` with it.
- Automatic alias resolution for plugin-inferred tasks ([§7.4](../../007.03-task-scoping-aliases-and-selectors.md#74-host-owned-task-scoping-and-aliases) order: compose, generate aliases from surviving declarations and explicit name occupancy, resolve chains, then inheritance). An explicit unqualified task suppresses the automatic alias at that name.
- An ambiguous automatic alias (two activations infer the same local name, no explicit occupant) stays an error listing both qualified candidates; list order never resolves it ([§7.3 decision 1](../../007.02-multiple-plugins-and-conflicts.md), [C20](../../007.04-composition-golden-fixtures.md#c20--alias-and-inheritance-failures-validate-surviving-graphs)).
- Tests start failing-first with synthetic contributor layers, as F07.3 did.
