# RFC 0024 implementation plan: plugin-ecosystem prerequisites

Work packages for the parts of [RFC 0024](../000.00-index.md) that are worth doing
**whether or not any plugin ever ships**. Each package is a bug fix, refactor or feature
that fixes an existing weakness or removes a limitation the plugin design depends on.
No package needs a plugin host, hook, Wasm engine or built-in adapter.

Everything plugin-specific (`omni_plugin*` crates, hooks, claims, native adapters,
Wasmtime, inference cache records, plugin feature flags) is **out of scope here** and
starts only after the packages it depends on land. See the
[RFC phases](../013.00-implementation-plan.md#133-phases) and
[readiness gates](../018.00-readiness-and-open-decisions.md#183-implementation-gates-grounded-in-current-code).

## Scheduling work in parallel

The packages below are briefings. For assigning work, use
[SCHEDULE.md](SCHEDULE.md) and [tasks.yaml](tasks.yaml): they split every package into
PR-sized **tasks** (e.g. `R04.2`, `P06.5`), with hard dependencies, conflict domains,
suggested worker lanes and generated waves. `python3 schedule.py` validates the graph.
The package-level waves further down are a coarse summary; the task graph supersedes them.

## How to use these files

- Each file is self-contained: a worker (human or subagent) can start from it plus the
  RFC sections it links. Read the file, then the linked RFC sections, then the code.
- IDs: `B` = bug fix, `R` = refactor, `F` = feature. A package is done when its
  **Acceptance** list passes, not when the code compiles.
- "Code starting points" were checked against the repository at planning time. Line
  numbers drift; search by symbol. Items marked _(locate)_ were not verified.
- Behavior corrections are intentional pre-v1 changes. Each needs a changelog /
  migration note and old-versus-new fixtures (the RFC calls this out per item).
- Do not widen scope. If you find adjacent work, add a note under the package's
  "Follow-ups" instead of doing it.
- Fixture names `C01`-`C26` refer to [§7.5](../007.04-composition-golden-fixtures.md).
  They are written as expected results, not runnable tests; turn the relevant ones into
  real tests inside the package that owns them.

## Status

Bug fixes B01-B04 and refactors R01-R08 are merged.
F01 and F02 are implemented together in the
[first feature batch](features/F01-F02-batch.md), pending PR review.
The remaining features (F03-F10) and plugins (P) have not been started.

Known gaps carried forward from the merged refactors:

- Virtual projects can be represented and guarded (R07.1, R07.3) but discovery cannot
  build one yet: `ProjectConfiguration.file` is still a required path (F07 or P05).
- The `alias` key is not parsed, so alias transitions (R05) are only tested with
  programmatically built aliases (F03).
- A project's own config file is listed twice as a cache-key input
  (`get_transitive_extendee_ids` includes the node itself). Kept so hashes stay
  stable; de-duplicating it changes cache keys and needs a High-risk review.
- `Interpolated` command operands apply value filters such as `upper` to the
  placeholder, not the bound value (R03).

## Packages

### Bug fixes (existing behavior is wrong or unsafe)

| ID                                                     | Title                                                        | Depends on | Size | Status                                                 |
| ------------------------------------------------------ | ------------------------------------------------------------ | ---------- | ---- | ------------------------------------------------------ |
| [B01](bug-fixes/B01-cache-lookup-honors-enabled.md)    | Cache lookup and replay ignore `cache.enabled: false`        | none       | S    | Done ([#83](https://github.com/omni-oss/omni/pull/83)) |
| [B02](bug-fixes/B02-glob-side-presence.md)             | Omitted glob include/exclude side decodes as empty replace   | none       | M    | Done ([#83](https://github.com/omni-oss/omni/pull/83)) |
| [B03](bug-fixes/B03-collection-envelope-validation.md) | Dictionary/list decoding can silently drop operator siblings | none       | M    | Done ([#83](https://github.com/omni-oss/omni/pull/83)) |
| [B04](bug-fixes/B04-uncached-dependency-demotion.md)   | Uncached dependencies feed a zero hash into cache keys       | B01        | M    | Done ([#85](https://github.com/omni-oss/omni/pull/85)) |

### Refactoring (restructure without intended behavior change)

| ID                                                        | Title                                                    | Depends on | Size | Status                                                                                                                                                                                |
| --------------------------------------------------------- | -------------------------------------------------------- | ---------- | ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [R01](refactoring/R01-extract-source-configurations.md)   | Extract `omni_source_configurations`                     | none       | S    | Done ([#88](https://github.com/omni-oss/omni/pull/88))                                                                                                                                |
| [R02](refactoring/R02-workspace-path-types.md)            | Shared `WorkspacePath` / `ProjectRootId` types           | none       | S    | Done ([#87](https://github.com/omni-oss/omni/pull/87))                                                                                                                                |
| [R03](refactoring/R03-literal-command-operands.md)        | Literal command operands separate from expressions       | none       | L    | Done ([#92](https://github.com/omni-oss/omni/pull/92)); wire/serde form for Wasm transport pending                                                                                    |
| [R04](refactoring/R04-presence-aware-layers.md)           | Presence-aware recursive configuration layers            | B02, B03   | XL   | Done: R04.1 [#92](https://github.com/omni-oss/omni/pull/92), R04.2-R04.4 [#94](https://github.com/omni-oss/omni/pull/94)                                                              |
| [R05](refactoring/R05-task-declaration-composition.md)    | Field-wise task composition and atomic alias transitions | R04        | M    | Done ([#95](https://github.com/omni-oss/omni/pull/95)); `alias` key not parsed yet (F03)                                                                                              |
| [R06](refactoring/R06-explicit-pattern-inputs.md)         | Explicit exact/pattern inputs in planner and API         | none       | M    | Done ([#89](https://github.com/omni-oss/omni/pull/89))                                                                                                                                |
| [R07](refactoring/R07-virtual-identity-and-raw-layers.md) | Virtual project identity and raw layer traversal         | R04        | L    | R07.1 [#94](https://github.com/omni-oss/omni/pull/94), R07.2 and R07.3 [#95](https://github.com/omni-oss/omni/pull/95); virtual projects cannot be built from discovery yet (F07/P05) |
| [R08](refactoring/R08-shared-local-cache-storage.md)      | Dependency-neutral local cache storage primitives        | none       | L    | Done ([#92](https://github.com/omni-oss/omni/pull/92))                                                                                                                                |

### Features (new capability useful on its own)

| ID                                                        | Title                                                   | Depends on       | Size |
| --------------------------------------------------------- | ------------------------------------------------------- | ---------------- | ---- |
| [F01](features/F01-duplicate-explicit-file-diagnostic.md) | Reject duplicate explicit project files at one root     | none             | S    |
| [F02](features/F02-protect-plugin-control-files.md)       | Projection guardrails for `plugin.omni.*`               | none             | XS   |
| [F03](features/F03-scoped-task-references.md)             | Structured scoped task identity and root references     | R05, R06         | L    |
| [F04](features/F04-exact-selectors-and-task-patterns.md)  | Exact positional selectors and `--task-pattern`         | R06, F03 (alias) | M    |
| [F05](features/F05-origin-aware-cache-tiers.md)           | Origin-aware cache enablement and activation `defaults` | R04, B04         | L    |
| [F06](features/F06-effective-semantic-digest.md)          | Per-task effective semantic digest in cache keys        | R03, R05, B01    | XL   |
| [F07](features/F07-conflict-detection-and-provenance.md)  | Composition conflict detection and provenance           | R04, R07         | L    |
| [F08](features/F08-source-acquisition-leases.md)          | Remote-source acquisition leases and atomic references  | none             | L    |
| [F09](features/F09-eligibility-inspection.md)             | Cache-eligibility and provenance inspection (CLI/MCP)   | B04, F05         | M    |
| [F10](features/F10-captured-input-view.md)                | Captured, containment-checked input view                | R02              | L    |

Size: XS < half a day, S ≈ 1-2 days, M ≈ 3-5 days, L ≈ 1-2 weeks, XL > 2 weeks
(split before starting; the file suggests slices).

## Dependency graph and waves

```text
Wave 0 (all independent, start in parallel)
  B01  B02  B03  R01  R02  R03  R06  R08  F01  F02  F08

Wave 1
  B04 <- B01
  R04 <- B02, B03
  F10 <- R02

Wave 2
  R05 <- R04
  R07 <- R04
  F05 <- R04, B04

Wave 3
  F03 <- R05, R06
  F04 <- R06, F03
  F06 <- R03, R05, B01 (and wants F05 for the final digest inputs)
  F07 <- R04, R07
  F09 <- B04, F05
```

Critical path: `B02/B03 → R04 → R05 → F03 → F04`, and `B01 → B04 → F05 → F06`.
Start R04 design early; it is the largest and most-depended-on package.

## Ordering rule from the RFC

[§13.3](../013.00-implementation-plan.md#133-phases) requires the execution-cache
fixes (**B01, B04, and the typed eligibility status inside B04/F05**) to land before any
plugin-specific host, adapter or runtime work. Treat B01 and B04 as the first merge
targets.

## Recommended PR/commit hygiene

- One package per PR (split XL packages along the slices listed in the file).
- Every behavior correction ships with its migration note and old/new fixture.
- Run the repository's normal Rust test, clippy and schema-regeneration commands for the
  touched crates; schema/serde parity matters for config-type changes.
- Update this README's table (add a `Status` column or tick marks) as packages land.

## Part 2: the plugin features

The plugin-specific packages (P01-P16: configuration, contract, host, inference cache,
pass integration, native JavaScript/Rust adapters, Wasm runtime, feature flags,
conformance, stabilization) live in [plugins/README.md](plugins/README.md), each gated on
the groundwork packages above. P01 (specification closure) can start immediately.

## Out of scope for part 1 (covered by part 2)

- `omni_plugin_configurations`, `omni_plugin`, `omni_plugin_host`,
  `omni_plugin_builtins`, runtime adapters, `plugins` workspace configuration,
  `inference` project controls, `apply`, hooks, claims/queries, inference cache.
- `@omni/javascript` / `@omni/rust` adapters and their specifications.
- Wasmtime, WIT, `wasm-rt`/`plugin-*` cargo features, benchmarks for them.
- Calibrated discovery budgets (§8.4) and hook/claim schema freezing (§6.4).

These need the packages above first and their own plan once the contract specs in
[§18.3](../018.00-readiness-and-open-decisions.md#183-implementation-gates-grounded-in-current-code)
are complete.
