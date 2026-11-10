# P11: `@omni/rust` native adapter (Cargo)

**Size:** L · **Depends on:** P01 (Cargo adapter spec), P06, P08

## Goal

Built-in native handler discovering Cargo packages/workspaces and inferring ordinary
Cargo tasks, without invoking Cargo, rustup or any toolchain.

## RFC references

- [§4.3 Cargo policy](../../004.01-native-ecosystem-adapters.md), [§13.3 Cargo fixture list](../../013.00-implementation-plan.md#133-phases)

## Slices

1. **Package vs aggregate classification:** package roots become candidates; aggregate/
   virtual workspace manifests are declined for projects but publish workspace-
   membership claims. Handle default-members and nested workspaces.
2. **Identity claims:** `package_identity` (system `cargo`), membership,
   `package_dependency` with structured registry/path/workspace/git request forms and
   combined version+path requests; no fabricated versions.
3. **Project edges:** confirmed workspace-local **normal and build** deps (including
   optional/target-specific, no platform/feature pruning); dev-only omitted but kept as
   workspace-resolution claims for conservative impact; dedupe targets; surviving
   cycles stay errors; bounded notices.
4. **Tasks:** `build`, `check`, `test` via structured argv with
   `--manifest-path Cargo.toml --package <cargo name>`, real project root as cwd, no
   `--workspace`/`--all*`, no task edges, no `cache.enabled`. No automatic `fmt`/
   `clippy`. No toolchain probing.
5. Partial workspaces: excluded confirmed local deps keep claims, no invalid edge.

## Acceptance

- Differing Cargo/omni names and renames keep package targeting correct.
- Multiple packages, nested workspaces, default-members fixtures.
- Dev cycles valid; included-edge cycles error with no arbitrary breaking.
- Native output matches contract fixtures (also the Wasm parity baseline).
