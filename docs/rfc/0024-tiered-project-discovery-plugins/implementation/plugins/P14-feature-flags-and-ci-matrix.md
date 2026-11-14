# P14: Cargo features and CI matrix

**Size:** M · **Depends on:** P02 (unsupported diagnostics, early), P13 (full)

## Goal

Compile-time features, default-on, that cost **nothing** when compiled out.

| Feature                  | Where                      | Effect                                                     |
| ------------------------ | -------------------------- | ---------------------------------------------------------- |
| `wasm-rt`                | Wasm runtime crate         | Compiles Wasmtime; nothing else depends on Wasmtime        |
| `plugin-wasm-rt`         | host/CLI/MCP/packaging     | Enables Wasm-backed plugins and turns on `wasm-rt`         |
| `plugin-native-builtins` | adapter + top-level crates | Compiles `@omni/javascript`, `@omni/rust` and registration |

## RFC references

- [§10 feature decision and verification](../../010.00-implementation-and-rollout.md)
- [§13.3 phase 3 feature bullet](../../013.00-implementation-plan.md#133-phases)

## Work

1. Make host, scheduler, inference cache, claim service, adapters and Wasmtime
   optional/`cfg`-gated. With neither plugin feature, host and contract crates are not
   linked.
2. Leftovers permitted only: the `plugins` schema entry and the rejection of a
   non-empty enabled list with a diagnostic naming the missing feature. Land this early
   (with P02) so the diagnostic exists before runtimes do.
3. Propagate features through CLI, MCP, packaging; defaults stay on in root/release.
4. CI jobs: default, no-`plugin-wasm-rt`, no-`plugin-native-builtins`, minimal.
   Assertions: `cargo tree` has no `wasmtime*` without `plugin-wasm-rt`; no host crates
   in minimal; stripped binary size within recorded thresholds per config; each
   unsupported-feature diagnostic fires while the other plugin kind still works.
5. Benchmarks for the minimal build show no regression vs pre-feature baseline.

## Acceptance

- All four configurations build and pass tests in CI.
- Size and dependency assertions are automated, with thresholds recorded in the repo.
- Building with a disabled feature never invokes a system interpreter.
