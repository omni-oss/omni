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

Bug fixes B01-B04 and refactors R01-R08 are merged. Of the standalone features, **F01, F02
and F08 are done**; F03, F05, F06 and F10 are partly done; F04 is done; F07 is not started and F09 is done except field provenance. The per-feature status is in the
[features table](#features-new-capability-useful-on-its-own), and the merged batches are:

- F01 and F02: the [first feature batch](features/F01-F02-batch.md) ([#101](https://github.com/omni-oss/omni/pull/101)).
- F03.1 and F06.1 (audits): the [prerequisite audit batch](features/F03-F06-audit-batch.md) ([#103](https://github.com/omni-oss/omni/pull/103)).
- F08.1 and F10.1 (designs): the [design batch](features/F08-F10-design-batch.md) ([#108](https://github.com/omni-oss/omni/pull/108)).
- F08.2 [lease primitive](features/F08.2-lease-primitive.md) ([#109](https://github.com/omni-oss/omni/pull/109)), F08.4 [atomic references and lockfile](features/F08.4-atomic-reference-api.md) ([#110](https://github.com/omni-oss/omni/pull/110)), F08.3 [lease-aware prune](features/F08.3-gc-honors-leases.md) ([#111](https://github.com/omni-oss/omni/pull/111)).
- F08.5 [migrating callers](features/F08.5-staged-installs.md): staged installs and explicit reference modes ([#112](https://github.com/omni-oss/omni/pull/112)), leases for runtime readers ([#113](https://github.com/omni-oss/omni/pull/113)), leases for install manifest readers ([#131](https://github.com/omni-oss/omni/pull/131)).
- F06.2 [canonical digest function](features/F06.2-canonical-digest.md) ([#114](https://github.com/omni-oss/omni/pull/114)); it is not wired into any cache key until F06.3.
- F05.1 [origin-tagged cache enablement](features/F05.1-origin-tagged-enablement.md) ([#115](https://github.com/omni-oss/omni/pull/115)).
- F03.2 [escape-aware reference parser](features/F03.2-reference-parser.md) ([#116](https://github.com/omni-oss/omni/pull/116)) and its use in task `dependencies`/`with` ([#117](https://github.com/omni-oss/omni/pull/117)).
- F03.3 [internal root-reference lowering](features/F03-scoped-task-references.md#f033-implementation-slice) ([#143](https://github.com/omni-oss/omni/pull/143)).

- F08.6 [`.pending` cleanup](features/F08.1-lease-gc-reference-protocol.md#f086-pending-cleanup) (D7). Leftovers from older omni versions (no lock file) are never reclaimed.

Open decisions that remain: store `VERSION` compatibility (D1), reference-driven retention
(D3). The F10.2 decisions D2, D5, D8 and D9 are recorded, as are the decision 1 amendment, the all-OS D9 scope, `max_observations_per_path` and view-local `DenyReason` ([confirmed decisions](features/F10-implementation-plan.md#confirmed-maintainer-decisions)); no maintainer confirmation is pending. Plugins (P) are not
started, except P01 (specification closure), which can start immediately; the rest wait on
the F03, F05 and F06 wiring.

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

| ID                                                        | Title                                                   | Depends on       | Size | Status                                                                                                                                                                                                                                                                                                                                                   |
| --------------------------------------------------------- | ------------------------------------------------------- | ---------------- | ---- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [F01](features/F01-duplicate-explicit-file-diagnostic.md) | Reject duplicate explicit project files at one root     | none             | S    | Done ([#101](https://github.com/omni-oss/omni/pull/101))                                                                                                                                                                                                                                                                                                 |
| [F02](features/F02-protect-plugin-control-files.md)       | Projection guardrails for `plugin.omni.*`               | none             | XS   | Done ([#101](https://github.com/omni-oss/omni/pull/101))                                                                                                                                                                                                                                                                                                 |
| [F03](features/F03-scoped-task-references.md)             | Structured scoped task identity and root references     | R05, R06         | L    | Partly done: F03.1–F03.3 are merged for their scoped deliverables. The handwritten `alias` key and plugin-scoped variants remain pending                                                                                                                                                                                                                 |
| [F04](features/F04-exact-selectors-and-task-patterns.md)  | Exact positional selectors and `--task-pattern`         | R06, F03 (alias) | M    | Done ([#134](https://github.com/omni-oss/omni/pull/134)): task, project, `hash` and `cache` selectors                                                                                                                                                                                                                                                    |
| [F05](features/F05-origin-aware-cache-tiers.md)           | Origin-aware cache enablement and activation `defaults` | R04, B04         | L    | Partly done: origin tiers ([#115](https://github.com/omni-oss/omni/pull/115)), the detector and `deciding_source` ([#130](https://github.com/omni-oss/omni/pull/130)), and [file provenance](features/F05.2-contradiction-detector.md#file-provenance-f052-prerequisite). Planner wiring, compatibility verification and rollout approval remain pending |
| [F06](features/F06-effective-semantic-digest.md)          | Per-task effective semantic digest in cache keys        | R03, R05, B01    | XL   | Partly done: F06.1 audit ([#103](https://github.com/omni-oss/omni/pull/103)), F06.2 digest function ([#114](https://github.com/omni-oss/omni/pull/114)). F06.3 (cache-key plumbing, High risk) is not started                                                                                                                                            |
| [F07](features/F07-conflict-detection-and-provenance.md)  | Composition conflict detection and provenance           | R04, R07         | L    | Not started (needs an approved plan)                                                                                                                                                                                                                                                                                                                     |
| [F08](features/F08-source-acquisition-leases.md)          | Remote-source acquisition leases and atomic references  | none             | L    | Done: F08.1-F08.5 ([#108](https://github.com/omni-oss/omni/pull/108)-[#113](https://github.com/omni-oss/omni/pull/113), [#131](https://github.com/omni-oss/omni/pull/131)). F08.6 (D7 `.pending` cleanup) done. Decisions D1 and D3 are still open                                                                                                       |
| [F09](features/F09-eligibility-inspection.md)             | Cache-eligibility and provenance inspection (CLI/MCP)   | B04, F05         | M    | Done except field provenance (needs F07): API operation, `omni cache eligibility` and the `cache_eligibility` MCP tool                                                                                                                                                                                                                                   |
| [F10](features/F10-captured-input-view.md)                | Captured, containment-checked input view                | R02              | L    | Partly done: F10.1 design ([#108](https://github.com/omni-oss/omni/pull/108)). F10.2-F10.3 have a [proposed plan](features/F10-implementation-plan.md) (needs approval; D2, D5, D8 and D9 recorded)                                                                                                                                                      |

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
- Update the relevant package status and briefing in the same PR that implements it.
  Mark the accepted slice done and link its implementation PR once it has a URL. If
  acceptance is only partial, list what shipped and what remains instead of marking
  the whole package done. A completed slice unblocks dependent work only after merge.

## Decisions for finishing the standalone features

Recorded before the plugin work resumes. Each still needs its own plan and, at High risk,
approval before it is built.

- **F05.2 planner error:** a hard error, as the RFC specifies. The Omni workspace audit finds
  no contradictions; other workspaces can be checked with `omni cache eligibility`.
- **F06.3:** platform stays out of the cache key (users can configure it explicitly); the
  workspace hash includes the digest; the one-time invalidation ships with a release note.
  Cache-key versioning and automatic migration need a separate design before v1.
- **F07:** two PRs, the provenance/metadata redesign first, then conflict detection.
- **F03:** the `alias` key lands together with its selector plumbing.
- **F10:** D2 symlinks are configurable and rejected by default (reject-only in the first
  release); D5 macOS returns `Unsupported` (no walk) until a verified beneath-root open
  exists; D9 rejects alternate streams and reserved names (on all OSes inside the view); D8 `CaseMismatch` on access, `Deleted` on revalidation;
  Linux uses a direct `openat2` and is `Unsupported` without it, with no fallback. See the
  [F10 plan](features/F10-implementation-plan.md) for the full decision block; no
  maintainer confirmation is pending.
- **F08 D1:** no store `VERSION` bump now; the gap is accepted. Automatic migration joins the
  pre-v1 cache-key versioning design. D3 is separate; D7 was implemented in F08.6.

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
