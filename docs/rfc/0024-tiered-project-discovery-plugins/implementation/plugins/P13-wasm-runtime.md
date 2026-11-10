# P13: WIT package and Wasmtime adapter

**Size:** XL (split) · **Depends on:** P06, P09, F10; native adapters (P10/P11) conformance green

## Goal

External plugins run as Wasm components under Wasmtime with no ambient authority, same
contract and output as native handlers.

## RFC references

- [§10 selected engine/ABI, consequences, D3 evidence](../../010.00-implementation-and-rollout.md)
- [§19 prototype replication](../../019.00-d3-prototype-replication.md)
- [§12.5 runtime adapters](../../012.00-rust-trait-sketches.md), [§8.4](../../008.03-tier-specific-enforcement.md)

## Slices

1. **WIT package** for `project_discovery` v0 from §6.4: plain types, **no WASI, no guest
   `async`/streams**. Versioned with the capability.
2. **Reusable engine core** (module/engine/pooling, instance ownership, limits,
   cancellation) separate from discovery bindings and profile-specific host services.
   No generator/tool integrations; no empty future-consumer crates.
3. **Adapter crate** `omni_plugin_runtime_wasm`: shared engine, compiled-component
   cache, linker + `InstancePre` built once per component, pooling allocator, fresh
   store per invocation. Synchronous guest hooks under async Wasmtime; **every host
   import async, cancellable, deadline- and budget-accounted**; cancel by dropping the
   future.
4. **Limits:** fuel, epoch interruption, memory caps, host-side bounded allocation/I/O;
   typed overflow classification.
5. **Import-set conformance:** stripped components import only authorized omni
   interfaces; WASI/unknown imports rejected. Pin and cross-test Wasmtime, wasm-tools,
   wit-bindgen versions.
6. **Artifacts:** load only the captured, hashed bytes (P09); precompiled-artifact
   caching keyed by digest + engine config.
7. **Escape hatch:** keep claim/query/budget logic independent of bindings.

## Acceptance

- Infinite loop, allocation/output/read floods, path traversal, symlink escape,
  forbidden imports, denied host ops all fail within budget with the host usable after.
- Blocked host call cancelled near the deadline (prototype shape: ~50 ms for 50 ms).
- Native and Wasm outputs identical for equivalent plugins across all H-fixtures
  (artifact identities may differ).
- Fresh vs reused instances and retries preserve normalized results.
- A Rust test plugin built with the documented toolchain is part of the repo fixtures.
