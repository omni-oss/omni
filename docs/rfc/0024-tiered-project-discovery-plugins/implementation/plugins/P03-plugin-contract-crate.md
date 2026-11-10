# P03: `omni_plugin` runtime-neutral contract crate

**Size:** L · **Depends on:** P01, P02, R02 · **Unblocks:** P06, P10, P11, P13

## Goal

Typed Rust contract that native handlers implement and runtime adapters (Wasm later)
translate to: requests, results, state views/updates, host-service interfaces, errors,
and typed capability registration with negotiation. No engine dependency.

## RFC references

- [§12 Rust trait sketches](../../012.00-rust-trait-sketches.md) (illustrative, not wire)
- [§6.4 schemas](../../006.04-hook-and-claim-schemas.md), [§6.1 lifecycle](../../006.00-hooks-and-lifecycle.md#61-hook-namespaces-and-lifecycle)
- [§14.2 negotiation](../../014.00-versioning-compatibility-and-migrations.md#142-negotiation), [§14.3 protocol vs SDK](../../014.00-versioning-compatibility-and-migrations.md#143-external-protocol-versus-rust-sdk)

## Work

1. New crate `crates/omni_plugin` (depends on P02 types, `omni_configurations`,
   R02 path types). No Wasmtime/JS imports.
2. Separate typed handlers per capability (no single `capability_id()`); a registry
   that validates declared hooks against the handler's supported set (misspelled names,
   inconsistent exports are errors).
3. Request/result types for `on_workspace`, `filter_candidates`, `infer_roots`,
   `resolve_dependencies`, `observe`, including tagged `accepted`/`declined` inference
   results and the resolution-only declaration.
4. `ProjectContribution` shape (generic over/placeholder for the contribution payload).
   **Task P03.2** wires in the restricted contribution types once P04.2 lands; P04.2
   owns the restriction validators. P03 itself does not depend on P04.
5. `StateValue`/`StateUpdate` (`keep`/`replace`/`clear`; omission = keep), `Notice`,
   `Cursor`, claim types (package identity, workspace membership, package dependency),
   query request/response with paging.
6. Host-service trait views: tracked reads/exists/list, claim query, cancellation,
   budget; async and cancellable.
7. Typed error taxonomy including transport-overflow classification (not decided from
   guest text).
8. Negotiation: joint version selection with deterministic tie-breaks; required
   vs optional capabilities, feature requirements.
9. Serde with `deny_unknown_fields`; round-trip for the same logical types the WIT
   package (P13) will mirror.

## Acceptance

- H-cases (from P01) that concern pure typing/validation pass against these types.
- Negotiation tests: ranges, missing features, optional skip, envelope fallback,
  overclaiming manifests.
- Fixed records vs dynamic maps, duplicate alias fields, accepted/declined shape errors.
- `cargo tree` shows no runtime-engine dependency.
