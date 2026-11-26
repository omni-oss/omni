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

Bug fixes B01-B04 and refactors R01-R08 are merged. Of the standalone features, **F01, F02, F04,
F08 and F10 are done** (F10 consumer wiring remains, see [What remains](#what-remains)); F03, F05, F06 and F07 are partly done; F09 is done except field provenance (needs F07.5). The per-feature status is in the
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

- F08.6 [`.pending` cleanup](features/F08.1-lease-gc-reference-protocol.md#f086-pending-cleanup) (D7) ([#165](https://github.com/omni-oss/omni/pull/165)). Leftovers from older omni versions (no lock file) are never reclaimed.
- F09 [cache-eligibility inspection](features/F09-eligibility-inspection.md) and the F05.2 [planner error](features/F05.2-contradiction-detector.md#planner-wiring-done) ([#160](https://github.com/omni-oss/omni/pull/160)).
- F03.4 [slice 0](features/F03.4-slice-0-task-identity-plan.md): structured `(project, task)` identity and the versioned identity-leaf v1 cache key, a deliberate cold start ([#171](https://github.com/omni-oss/omni/pull/171)); plan [#167](https://github.com/omni-oss/omni/pull/167).
- F07.1 and F07.2 [provenance replay and project location](features/F07-implementation-plan.md) ([#172](https://github.com/omni-oss/omni/pull/172)).
- F03.4 [slices 1-4](features/F03.4-authored-alias-plan.md#build-result): authored `alias` key, canonical selection, alias hints ([#175](https://github.com/omni-oss/omni/pull/175)).
- F03.5 [`with` sibling prerequisites](features/F03.5-with-sibling-prerequisites-plan.md): a task added through `with` now gets its own prerequisites scheduled, in one fixed-point expansion of the batch planner ([#178](https://github.com/omni-oss/omni/pull/178)).
- F10.2a [containment primitive and proof fixture](features/F10-implementation-plan.md#f102a-implementation-record), crate `omni_input_view`, nothing consumes it yet ([#174](https://github.com/omni-oss/omni/pull/174)); plan [#169](https://github.com/omni-oss/omni/pull/169).
- F10.2b [tracked `read`/`exists`/`list` with limits and benchmark](features/F10-implementation-plan.md#f102b-implementation-record), F10.2c [reject-only symlink policy](features/F10-implementation-plan.md#f102c-implementation-record) and F10.3 [revalidation and reject-on-change](features/F10-implementation-plan.md#f103-implementation-record), all in crate `omni_input_view` ([#176](https://github.com/omni-oss/omni/pull/176)). This completes F10; nothing consumes the view yet.
- F10 diagnostics ([#179](https://github.com/omni-oss/omni/pull/179)): `InputError` is an `omni_diagnostic` diagnostic under the prefix `input_view` (stable codes and hints), every failure names the workspace-relative path and the reason, and the post-merge maintainer decisions are recorded in the [plan](features/F10-implementation-plan.md#diagnostics-and-maintainer-decisions-after-merge).
- [RFC 0032](../../0032-pre-v1-migration-infrastructure.md) pre-v1 migration infrastructure: plan ([#166](https://github.com/omni-oss/omni/pull/166), follow-ups [#168](https://github.com/omni-oss/omni/pull/168)), slice 1 contract-proof harness ([#170](https://github.com/omni-oss/omni/pull/170)), slices 2 and 3, the unwired `crates/omni_migration` ([#173](https://github.com/omni-oss/omni/pull/173)). Not adopted by any production store.

Open decisions that remain: reference-driven retention (D3); D1 is decided (no bump now, see below). The F10.2 decisions D2, D5, D8 and D9 are recorded, as are the decision 1 amendment, the all-OS D9 scope, `max_observations_per_path` and view-local `DenyReason` ([confirmed decisions](features/F10-implementation-plan.md#confirmed-maintainer-decisions)); no maintainer confirmation is pending. Plugins (P) are not
started, except P01 (specification closure), which can start immediately; the rest wait on
the F03, F05 and F06 wiring.

Known gaps carried forward from the merged refactors:

- Virtual projects can be represented and guarded (R07.1, R07.3) but discovery cannot
  build one yet: a finalized project may have no file (`FinalizedProject::file` is an `Option`, F07.2 moved `file`/`dir` out of `ProjectConfiguration`), but discovery and `ProjectConfigEntry::from_finalized` only produce file-backed entries (discovery work is P05). The extractor's fileless branch has no test through `extract_all` (#172).
- F03.5 [`with` sibling prerequisites](features/F03.5-with-sibling-prerequisites-plan.md) is fixed: the prerequisites of `with` tasks are scheduled. Sorting tasks within a batch is done (see the [follow-up](features/F03.5-with-sibling-prerequisites-plan.md#follow-up-sort-tasks-within-a-batch)). The `--with-dependents` name-only matching is investigated in [F03.6](features/F03.6-with-dependents-plan.md).
- The `alias` key is now parsed (F03.4, #175). Not covered there: an alias-variant benchmark fixture, a CLI-level check that stale/orphan alias hints survive preview and confirm, and F07 `cache.enabled` provenance through an alias-bearing `extends` (all listed in #175).
- Cache keys use a versioned identity leaf (F03.4 slice 0, #171), so a first run after upgrading is a cold start. Gaps accepted in #171: no decoded `cache.meta.bin` snapshot, no remote end-to-end test beyond the fake-remote Rust tests.
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

| ID                                                        | Title                                                    | Depends on | Size | Status                                                                                                                                                                                                                                                                                           |
| --------------------------------------------------------- | -------------------------------------------------------- | ---------- | ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| [R01](refactoring/R01-extract-source-configurations.md)   | Extract `omni_source_configurations`                     | none       | S    | Done ([#88](https://github.com/omni-oss/omni/pull/88))                                                                                                                                                                                                                                           |
| [R02](refactoring/R02-workspace-path-types.md)            | Shared `WorkspacePath` / `ProjectRootId` types           | none       | S    | Done ([#87](https://github.com/omni-oss/omni/pull/87))                                                                                                                                                                                                                                           |
| [R03](refactoring/R03-literal-command-operands.md)        | Literal command operands separate from expressions       | none       | L    | Done ([#92](https://github.com/omni-oss/omni/pull/92)); wire/serde form for Wasm transport pending                                                                                                                                                                                               |
| [R04](refactoring/R04-presence-aware-layers.md)           | Presence-aware recursive configuration layers            | B02, B03   | XL   | Done: R04.1 [#92](https://github.com/omni-oss/omni/pull/92), R04.2-R04.4 [#94](https://github.com/omni-oss/omni/pull/94)                                                                                                                                                                         |
| [R05](refactoring/R05-task-declaration-composition.md)    | Field-wise task composition and atomic alias transitions | R04        | M    | Done ([#95](https://github.com/omni-oss/omni/pull/95)); the `alias` key is parsed since F03.4 ([#175](https://github.com/omni-oss/omni/pull/175))                                                                                                                                                |
| [R06](refactoring/R06-explicit-pattern-inputs.md)         | Explicit exact/pattern inputs in planner and API         | none       | M    | Done ([#89](https://github.com/omni-oss/omni/pull/89))                                                                                                                                                                                                                                           |
| [R07](refactoring/R07-virtual-identity-and-raw-layers.md) | Virtual project identity and raw layer traversal         | R04        | L    | R07.1 [#94](https://github.com/omni-oss/omni/pull/94), R07.2 and R07.3 [#95](https://github.com/omni-oss/omni/pull/95); `file`/`dir` moved out of `ProjectConfiguration` in F07.2 ([#172](https://github.com/omni-oss/omni/pull/172)); virtual projects cannot be built from discovery yet (P05) |
| [R08](refactoring/R08-shared-local-cache-storage.md)      | Dependency-neutral local cache storage primitives        | none       | L    | Done ([#92](https://github.com/omni-oss/omni/pull/92))                                                                                                                                                                                                                                           |

### Features (new capability useful on its own)

| ID                                                        | Title                                                   | Depends on       | Size | Status                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| --------------------------------------------------------- | ------------------------------------------------------- | ---------------- | ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| [F01](features/F01-duplicate-explicit-file-diagnostic.md) | Reject duplicate explicit project files at one root     | none             | S    | Done ([#101](https://github.com/omni-oss/omni/pull/101))                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| [F02](features/F02-protect-plugin-control-files.md)       | Projection guardrails for `plugin.omni.*`               | none             | XS   | Done ([#101](https://github.com/omni-oss/omni/pull/101))                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| [F03](features/F03-scoped-task-references.md)             | Structured scoped task identity and root references     | R05, R06         | L    | Partly done: F03.1–F03.3 are merged; F03.4 (structured `(project, task)` identity and the versioned cache-key identity leaf, [#171](https://github.com/omni-oss/omni/pull/171); the authored `alias` key, canonical selection and alias hints, [#175](https://github.com/omni-oss/omni/pull/175)) is merged. Plugin-scoped variants remain pending                                                                                                                                                                       |
| [F04](features/F04-exact-selectors-and-task-patterns.md)  | Exact positional selectors and `--task-pattern`         | R06, F03 (alias) | M    | Done ([#134](https://github.com/omni-oss/omni/pull/134)): task, project, `hash` and `cache` selectors                                                                                                                                                                                                                                                                                                                                                                                                                    |
| [F05](features/F05-origin-aware-cache-tiers.md)           | Origin-aware cache enablement and activation `defaults` | R04, B04         | L    | Partly done: origin tiers ([#115](https://github.com/omni-oss/omni/pull/115)), the detector and `deciding_source` ([#130](https://github.com/omni-oss/omni/pull/130)), and [file provenance](features/F05.2-contradiction-detector.md#file-provenance-f052-prerequisite). The planner error is wired ([#160](https://github.com/omni-oss/omni/pull/160): an explicit `cache.enabled` contradiction fails planning; Omni's own workspace has 0 contradictions). Compatibility checks on other workspaces are not recorded |
| [F06](features/F06-effective-semantic-digest.md)          | Per-task effective semantic digest in cache keys        | R03, R05, B01    | XL   | Partly done: F06.1 audit ([#103](https://github.com/omni-oss/omni/pull/103)), F06.2 digest function ([#114](https://github.com/omni-oss/omni/pull/114)). F06.3 (cache-key plumbing, High risk) is not started                                                                                                                                                                                                                                                                                                            |
| [F07](features/F07-conflict-detection-and-provenance.md)  | Composition conflict detection and provenance           | R04, R07         | L    | Partly done: plan approved ([#164](https://github.com/omni-oss/omni/pull/164)); F07.1 (replayed `cache.enabled` value and provenance, typed `DeclarationSource`) and F07.2 (`file`/`dir` moved out of `ProjectConfiguration` into `FinalizedProject`) merged in [#172](https://github.com/omni-oss/omni/pull/172). F07.3-F07.5 are not started                                                                                                                                                                           |
| [F08](features/F08-source-acquisition-leases.md)          | Remote-source acquisition leases and atomic references  | none             | L    | Done: F08.1-F08.5 ([#108](https://github.com/omni-oss/omni/pull/108)-[#113](https://github.com/omni-oss/omni/pull/113), [#131](https://github.com/omni-oss/omni/pull/131)). F08.6 (D7 `.pending` cleanup) done ([#165](https://github.com/omni-oss/omni/pull/165)). D1 is decided (no `VERSION` bump now); D3 is still open                                                                                                                                                                                              |
| [F09](features/F09-eligibility-inspection.md)             | Cache-eligibility and provenance inspection (CLI/MCP)   | B04, F05         | M    | Done except field provenance (needs F07.5): API operation, `omni cache eligibility` and the `cache_eligibility` MCP tool ([#160](https://github.com/omni-oss/omni/pull/160))                                                                                                                                                                                                                                                                                                                                             |
| [F10](features/F10-captured-input-view.md)                | Captured, containment-checked input view                | R02              | L    | Done ([#108](https://github.com/omni-oss/omni/pull/108) design, [#169](https://github.com/omni-oss/omni/pull/169) plan, [#174](https://github.com/omni-oss/omni/pull/174) F10.2a, [#176](https://github.com/omni-oss/omni/pull/176) F10.2b, F10.2c and F10.3, [#179](https://github.com/omni-oss/omni/pull/179) diagnostics), see the [plan](features/F10-implementation-plan.md#f103-implementation-record). Crate `omni_input_view`; nothing consumes it yet, so there is no user-visible change                       |

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

- **F05.2 planner error:** a hard error, as the RFC specifies; implemented in [#160](https://github.com/omni-oss/omni/pull/160). The Omni workspace audit finds
  no contradictions (1,027 tasks); other workspaces can be checked with `omni cache eligibility`.
- **F06.3:** platform stays out of the cache key (users can configure it explicitly); the
  workspace hash includes the digest; the one-time invalidation ships with a release note.
  Cache-key versioning and automatic migration need a separate design before v1.
- **F07:** the provenance/metadata redesign first (F07.1 and F07.2, merged in [#172](https://github.com/omni-oss/omni/pull/172)), then conflict detection (F07.3-F07.5, not started). The plan now has five slices; see the [F07 plan](features/F07-implementation-plan.md).
- **F03:** the `alias` key landed together with its selector plumbing ([#175](https://github.com/omni-oss/omni/pull/175)). Decisions recorded in the [plan](features/F03.4-authored-alias-plan.md) ([#167](https://github.com/omni-oss/omni/pull/167)): structured `(project, task)` identity internally, escaped `project#task` labels only at boundaries, a deliberate versioned cache-key break with a cold start and no legacy promotion ([#171](https://github.com/omni-oss/omni/pull/171)), the shared escaped grammar for authored task keys, preserved wire formats, and direct canonical matching for inventory `--dir`/`--meta`.
- **RFC 0032 (pre-v1 migration infrastructure):** the contract is proven on two fixtures and extracted as the unwired `crates/omni_migration` ([#170](https://github.com/omni-oss/omni/pull/170), [#173](https://github.com/omni-oss/omni/pull/173)). Chain identity binding and a backend capability gate were added in [#168](https://github.com/omni-oss/omni/pull/168). Production adoption needs a separate design and release plan.
- **F10:** D2 symlinks are configurable and rejected by default (reject-only in the first
  release); D5 macOS returns `Unsupported` (no walk) until a verified beneath-root open
  exists; D9 rejects alternate streams and reserved names (on all OSes inside the view); D8 `CaseMismatch` on access, `Deleted` on revalidation;
  Linux uses a direct `openat2` and is `Unsupported` without it, with no fallback. See the
  [F10 plan](features/F10-implementation-plan.md) for the full decision block; no
  maintainer confirmation is pending.
- **F08 D1:** no store `VERSION` bump now; the gap is accepted. Automatic migration joins the
  pre-v1 cache-key versioning design. D3 is separate; D7 was implemented in F08.6 ([#165](https://github.com/omni-oss/omni/pull/165)).

## What remains

Derived from the merged PRs above; each item needs its own plan and, at High risk, approval.

- **F07.3-F07.5:** unordered contributor composition with conflict detection, conflict diagnostics, and winning provenance through `omni cache eligibility` and the `cache_eligibility` MCP tool (this also closes F09 field provenance). See the [F07 plan](features/F07-implementation-plan.md).
- **F06.3:** plumbing the digest into cache keys (platform stays out, workspace hash includes the digest, release note). It needs the pre-v1 cache-key versioning design first.
- **RFC 0032 production adoption:** `omni_migration` is unwired and no store uses it. Adoption needs a separate design and release plan (mixed binaries, backups and recovery, compatibility, rollback). Not proven: power loss, network filesystems, platforms other than Linux, async cancellation, uncooperative writers, mixed binaries ([#173](https://github.com/omni-oss/omni/pull/173)).
- **F10 consumer wiring:** F10 is fully merged ([#174](https://github.com/omni-oss/omni/pull/174), [#176](https://github.com/omni-oss/omni/pull/176), diagnostics in [#179](https://github.com/omni-oss/omni/pull/179)) but nothing in omni calls `omni_input_view` yet. The first consumer must make `InputLimits` user-configurable within host-owned bounds ([#169](https://github.com/omni-oss/omni/pull/169)), owns the release note, and should: adapt `omni_capabilities` to `InputAuthorizer`; decide which of the path-bearing messages a plugin-facing layer passes on; emit a notice when it continues after a failed read, so the user sees which input was skipped and why; and add a CLI end-to-end test of the `input_view.*` codes and exit statuses. Native results: the `omni_input_view` test task passed on Linux, Windows and macOS for [#174](https://github.com/omni-oss/omni/pull/174) and, at head `4226caea`, for [#176](https://github.com/omni-oss/omni/pull/176) on all five native targets (the first Windows run of #176 failed one test, fixed in `f71f3e5f`; the CI log shows the task result, not per-test names). macOS stays `Unsupported` (the tests assert it) until beneath-root containment is verified on native macOS CI. Only x86_64 Linux proves Linux containment: the aarch64 `cross` container blocks `openat2`, which is accepted and noted, and the tests there assert `Unsupported`.
- **`with` follow-ups (F03.5, [#178](https://github.com/omni-oss/omni/pull/178)):** the gap itself is fixed, sorting tasks within a batch is done ([#182](https://github.com/omni-oss/omni/pull/182)), and the order of the `--result` list (and of the API and MCP `results`) is now sorted by `(project, task)` ([#183](https://github.com/omni-oss/omni/pull/183)). What remains is `--with-dependents`, which matches dependents by task name only. It is investigated in [F03.6](features/F03.6-with-dependents-plan.md): same-name matching is intended and documented; gap 1 (the search stopped at a differently named task) is fixed in [#184](https://github.com/omni-oss/omni/pull/184), and gap 2 (aliases) stays per project.
- **F03 plugin-scoped variants** and **F08 D3** (reference-driven retention).

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
