# P10: `@omni/javascript` native adapter

**Size:** XL (split) · **Depends on:** P01 (JS adapter spec), P06, P08

## Goal

Built-in native handler discovering JavaScript/TypeScript projects from `package.json`
for npm, Yarn, pnpm and Bun, behind the `project_discovery` capability. Manifest roots
with tracked lockfile-assisted inference; never lockfile-only project creation.

## RFC references

- [§4.3](../../004.01-native-ecosystem-adapters.md) (policies), [§13.3 phase 2 fixture lists](../../013.00-implementation-plan.md#133-phases)
- [§16 walkthrough](../../016.00-native-plugin-walkthrough.md)

## Slices

1. **Crate scaffold** `omni_plugin_builtins` (JS module) + registration behind
   `plugin-native-builtins`. Handler implements typed `project_discovery` only.
2. **Ownership and workspace context:** standalone packages, confirmed members,
   genuine nested workspace boundaries, irrelevant ancestor locks ignored. Fixtures
   written **before** detection code.
3. **Manager selection:** explicit option → `packageManager` → unique lockfile → npm.
   Conflicting locks fail unless a stronger choice resolves them; unsupported selected
   declarations and required-read failures are fatal; tracked negative reads; no manager
   execution.
4. **Naming and claims:** declared name; fallback to workspace-relative path; no
   fabricated `package_identity` for the fallback; `package_identity`, membership and
   `package_dependency` claims.
5. **Project edges:** local normal/dev/optional deps via ecosystem resolution evidence
   (not name matching), same-name externals, deduped targets, partial-workspace handling
   (retain claim, no invalid edge, bounded notice), peers per spec.
6. **Tasks:** one task per valid script (manager-runner command, literal script name via
   R03), custom and lifecycle scripts included, no automatic task edges, no execution of
   lifecycle scripts. Manifest edits must invalidate task reuse even with
   `cache.key.defaults: false` (manifest included as input contribution).
7. **No `cache.enabled` emitted**; adapter input/output policy per spec.

## Acceptance

- All spec fixtures for npm/Yarn/pnpm/Bun, ambiguity and failure paths.
- Script names resembling flags/`$VAR`/Tera/delimiters execute unmodified.
- Parse, permission and resource failures remain fatal; no manager is ever spawned.
- Native output equals the contract fixtures exactly (reuse for Wasm parity).
