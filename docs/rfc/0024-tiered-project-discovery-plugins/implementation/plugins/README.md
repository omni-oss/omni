# Plugin phases: the features themselves

Second half of the [implementation plan](../README.md). The groundwork packages
(B/R/F, in the sibling folders) must land first. These `P` packages build the plugin
system itself, in the order of [RFC §13.3 phases 2-5](../../013.00-implementation-plan.md#133-phases).

**Rule that does not bend:** no plugin-specific host, adapter or runtime work starts
until its groundwork gate (below) is merged. Several gates are in the RFC's own text:
cache-eligibility fixes (B01, B04, F05) come before everything here.

## Packages

### Phase A: contract, configuration and composition (no runtime)

| ID                                                 | Title                                                                       | Groundwork gate                        | Size |
| -------------------------------------------------- | --------------------------------------------------------------------------- | -------------------------------------- | ---- |
| [P01](P01-contract-and-adapter-specs.md)           | Close the specification gaps (contract, fixtures, adapters)                 | none (can start now, in parallel)      | L    |
| [P02](P02-plugin-configuration-crate.md)           | `omni_plugin_configurations` and `plugins` workspace config                 | R01                                    | M    |
| [P03](P03-plugin-contract-crate.md)                | `omni_plugin` runtime-neutral contract crate                                | P01, P02, R02 (P03.2 later adds P04.2) | L    |
| [P04](P04-project-inference-controls-and-apply.md) | Project `inference` controls, `apply`, `defaults`, restricted contributions | R04, R05, F03, F05, F07, P02           | XL   |
| [P05](P05-marker-indexing-and-discovery.md)        | Marker indexing, candidate grouping, virtual-root loading                   | R07, F01, P02                          | L    |

### Phase B: host and infrastructure

| ID                                                   | Title                                                         | Groundwork gate                 | Size |
| ---------------------------------------------------- | ------------------------------------------------------------- | ------------------------------- | ---- |
| [P06](P06-plugin-host.md)                            | `omni_plugin_host`: scheduling, claims, transactions, budgets | P03.2, P05, F05.2 (cache fixes) | XL   |
| [P07](P07-inference-cache.md)                        | Inference cache (records, indexes, publication)               | R08, P06                        | L    |
| [P08](P08-pass-integration-and-semantic-identity.md) | Full-pass integration into `Context` and execution identity   | F06, F10, P04, P06, P07         | L    |
| [P09](P09-plugin-sources-and-artifacts.md)           | Plugin source contributor, manifests, captured artifacts      | F08, F02, P02                   | L    |

### Phase C: native pilots

| ID                                      | Title                                           | Gate                       | Size |
| --------------------------------------- | ----------------------------------------------- | -------------------------- | ---- |
| [P10](P10-native-javascript-adapter.md) | `@omni/javascript` (npm, Yarn, pnpm, Bun)       | P01 adapter spec, P06, P08 | XL   |
| [P11](P11-native-rust-adapter.md)       | `@omni/rust` (Cargo)                            | P01 adapter spec, P06, P08 | L    |
| [P12](P12-plugin-inspection-cli-mcp.md) | Plugin inspection, notices and CLI/MCP surfaces | P08, F09                   | M    |

### Phase D: Wasm runtime, features, release

| ID                                        | Title                                                        | Gate                                    | Size |
| ----------------------------------------- | ------------------------------------------------------------ | --------------------------------------- | ---- |
| [P13](P13-wasm-runtime.md)                | WIT package and Wasmtime adapter                             | P06, P09, F10; native conformance green | XL   |
| [P14](P14-feature-flags-and-ci-matrix.md) | `wasm-rt` / `plugin-wasm-rt` / `plugin-native-builtins` + CI | P02 (diagnostics), P13 (full)           | M    |
| [P15](P15-conformance-and-benchmarks.md)  | Conformance suite, budgets and benchmarks                    | P10, P11, P13                           | L    |
| [P16](P16-stabilization-and-compat.md)    | Versioning, migration tooling, SDK docs, 1.0 freeze          | P15                                     | L    |
| P17 (deferred)                            | JavaScript plugin runtime                                    | not part of this RFC's v1               | n/a  |

## Waves

```text
Wave P0 (start now, no code dependency)
  P01 (spec closure; feeds everything)

Wave P1 (after groundwork R01/R02 and P01 drafts)
  P02 -> P03
  P05 (needs R07, F01)

Wave P2 (needs R04/R05/F03/F05/F07)
  P04 (largest; slice it)
  P09 (needs F08, F02)

Wave P3
  P06 host        <- P03, P05
  P07 inf. cache  <- R08, P06
  P08 pass wiring <- F06, F10, P04, P06, P07

Wave P4 (parallel adapters, each gated on its own spec from P01)
  P10 JavaScript    P11 Rust    P12 inspection

Wave P5
  P13 Wasm -> P14 flags/CI (P14 minimal part lands with P02) -> P15 -> P16
```

## Cross-cutting rules for every P package

1. **Spec before types.** Hook/claim/query payloads (§6.4), adapter tables and the
   fixture cases (H01-H24, C01-C26) are inputs, not afterthoughts. Convert the Markdown
   cases into executable fixtures inside the package that implements the behavior.
2. **Host owns policy.** Plugins return data; the host validates, composes, caches and
   enforces permissions, budgets and cancellation. A plugin never gets write, process
   or network authority.
3. **Explicit activation only.** `plugins: []` by default. Nothing is implicitly
   enabled, fetched or installed, by a manifest, dependency or built-in.
4. **No-plugin cost is zero.** No plugin hook, scan, index, cache or allocation work
   when no plugin is enabled; default behavior equals the groundwork's zero-plugin
   behavior. Verify with benchmarks each time a package touches hot paths.
5. **Native/Wasm parity.** Native handlers use the same logical types; later Wasm must
   produce identical normalized output, provenance and validation outcomes.
6. **Failures are fatal, not empty.** Permission, parse, budget, incompatibility and
   producer failures never degrade to an empty result or stale fallback.
7. **Behavior corrections** from the groundwork have migration notes already; plugin
   packages add none to zero-plugin behavior.
