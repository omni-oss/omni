# Parallel execution schedule

The packages (B/R/F/P) are too coarse to parallelize well: several are XL and hide
independent slices. This file and [`tasks.yaml`](tasks.yaml) split them into **tasks**
(one PR each) with explicit hard dependencies, so any number of workers, human or
subagent, can pick up whatever is unblocked.

- **Source of truth:** `tasks.yaml`. Edit it, then run `python3 schedule.py` to
  validate (unknown deps, cycles) and regenerate the wave tables below.
  `python3 schedule.py --check` is CI-safe. Both commands require PyYAML installed for
  `python3` (`pip install pyyaml`).
- **Task ids:** `B01`, `R04.2`, `P06.5`... The part before the dot is the parent
  package file (`parent` in the yaml); the slice description is in the yaml `title` and
  the parent file's Slices/Work section.
- Parent files remain the briefing for context, RFC links, code starting points and
  acceptance. A task's acceptance is the subset of the parent's acceptance matching its
  title.

## Rules for parallel work

1. **Start only when ready.** A task is ready when every id in `deps` is merged to the
   integration branch. Do not start early against an unmerged branch unless you own both
   tasks.
2. **Conflict domains.** Each task lists `touches`. Two ready tasks that share a domain
   (other than `docs`) edit the same crates/files; run them one after another or assign
   them to the same worker. Disjoint domains are safe in parallel. Domains are defined
   at the top of `tasks.yaml`.
3. **Spec and doc tasks (`kind: spec|doc`) never block on code** and rarely conflict:
   always start them first, and give them to a worker who will later implement the
   dependent code.
4. **One PR per task.** If a task grows beyond its size class, split it in
   `tasks.yaml` rather than widening it.
5. **Rebase, do not stack.** Keep tasks small enough to merge independently. If a
   dependent task needs unmerged code, wait or combine them deliberately in one PR.
6. **Claim before you start.** Record the claimant next to the task (add `owner:` in
   `tasks.yaml` or use the issue tracker) so two workers do not take the same task.
7. **Shared hotspots** (highest merge-conflict risk, merge these first in their wave):
   `project_data_extractor.rs`/`CacheInfo` (R04.3, F05.1, F06.3, P04.4, P08.3),
   `context.rs`/loader (R04.x, R07.x, F07, P04.3, P05, P08.x), `hybrid.rs`
   (R06, R08.x, F06.3).

## Lanes (suggested worker assignment)

Lanes are chains of tasks that share domains and knowledge, so one worker keeps context.
Different lanes run in parallel.

| Lane                 | Tasks (in order)                                                                       |
| -------------------- | -------------------------------------------------------------------------------------- |
| A. Cache correctness | B01 → B04 → F05.2 → F09                                                                |
| B. Config layers     | B02, B03 → R04.1 → R04.2 → R04.3 → R04.4 → R05 → F07                                   |
| C. Command/identity  | R03.1 → R03.2 → R03.3, R03.4 → F06.1 → F06.2 → F06.3                                   |
| D. Selection         | R06 → F03.1 → F03.2 → F03.3 → F04                                                      |
| E. Project identity  | R02 → R07.1 → R07.2 → R07.3; F01                                                       |
| F. Source/infra      | F08.1 → F08.2 → F08.3, F08.4 → F08.5; F02                                              |
| G. Storage/input     | R08.1 → R08.2 → R08.3; F10.1 → F10.2 → F10.3                                           |
| H. Specs             | P01.1 → P01.2, P01.3, P01.4; P01.5; R01 → P02 → P03                                    |
| I. Plugin config     | P04.1, P04.2 → P04.3 → P04.4 → P04.5; P05                                              |
| J. Host              | P06.1–P06.7 → P07 → P08.1 → P08.2 → P08.3 → P08.4                                      |
| K. Adapters          | P10.1–P10.7 (one worker); P11.1–P11.5 (second worker); P09.1–P09.3                     |
| L. Wasm              | P13.2 (can start any time) → P13.1 → P13.3 → P13.4, P13.5, P13.6 → P13.7; P14.1, P14.2 |
| M. Release           | P12; P15 → P16                                                                         |

Lanes B and I are the busiest; split B into two workers after R04.2 (loader owner and
fixtures/migration owner).

## Things that can start on day one

Every task with no dependency: B01, B02, B03, R01, R02, R03.1, R06, R08.1, F01, F02,
F03.1, F06.1, F08.1, P01.1, P01.5, P13.2 and the wave-0 rows below. That is ample work
for a large team before any shared hotspot is touched.

## Generated tables

Do not edit between the markers; run `python3 schedule.py`.

<!-- BEGIN GENERATED -->

Tasks: 97 · waves: 16 · widest wave: 16 · critical path ≈ 100 working days

**Critical path:** B02 → R04.1 → R04.2 → R04.3 → R07.3 → P05 → P06.2 → P06.5 → P06.6 → P08.1 → P08.2 → P08.4 → P10.7 → P13.7 → P15 → P16

## Waves

A task may start once every dependency is merged. All tasks in a wave are dependency-independent. `⚠` marks a conflict-domain overlap inside the wave (see the rules above).

### Wave 0

| Task  | Title                                            | Size | Deps | Domains                         |
| ----- | ------------------------------------------------ | ---- | ---- | ------------------------------- |
| B01   | Cache lookup/replay honors cache.enabled         | S    | -    | executor                        |
| B02   | Glob include/exclude side presence               | M    | -    | glob                            |
| B03   | Strict collection envelope validation            | M    | -    | collections                     |
| F01   | Duplicate explicit project file diagnostic       | S    | -    | discovery                       |
| F02   | Projection guardrails for plugin.omni.*          | XS   | -    | -                               |
| F03.1 | Identity-keyed map and parser audit              | S    | -    | docs                            |
| F06.1 | Effective-field audit note                       | M    | -    | docs                            |
| F08.1 | Lease/GC/reference protocol doc                  | S    | -    | docs                            |
| P01.1 | Complete hook/claim/query schemas                | L    | -    | docs                            |
| P01.5 | Budget/counter design (candidates)               | M    | -    | docs                            |
| P13.2 | Reusable Wasm engine core                        | L    | -    | plugin-wasm                     |
| R01   | Extract omni_source_configurations               | S    | -    | config-types                    |
| R02   | WorkspacePath / ProjectRootId types              | S    | -    | -                               |
| R03.1 | Command-operand audit note                       | S    | -    | docs                            |
| R06   | Explicit exact/pattern inputs in planner and API | M    | -    | planner, cache-store, collector |
| R08.1 | Cache storage audit note                         | S    | -    | docs                            |

### Wave 1

| Task    | Title                                          | Size | Deps       | Domains                     |
| ------- | ---------------------------------------------- | ---- | ---------- | --------------------------- |
| B04     | Uncached-dependency demotion + typed status    | M    | B01        | executor                    |
| F03.2 ⚠ | Scoped reference schema, parser/formatter      | M    | F03.1, R06 | config-types, planner       |
| F08.2 ⚠ | Process-held lease primitive                   | M    | F08.1      | source                      |
| F08.4 ⚠ | Atomic reference API (complete vs additive)    | M    | F08.1      | source                      |
| F10.1   | Captured input view design doc                 | M    | R02        | docs                        |
| P01.2   | Executable H-case fixtures + validator harness | M    | P01.1      | docs                        |
| P01.3   | JavaScript adapter spec + fixtures             | L    | P01.1      | docs                        |
| P01.4   | Cargo adapter spec + fixtures                  | L    | P01.1      | docs                        |
| P02 ⚠   | omni_plugin_configurations + plugins config    | M    | R01        | plugin-config, config-types |
| R03.2   | Literal/expression operand type design         | M    | R03.1      | command                     |
| R04.1 ⚠ | Layer types with presence                      | L    | B02, B03   | config-types                |
| R07.1   | Logical project key + guards against file use  | M    | R02        | loader                      |
| R08.2   | Neutral local storage API + test consumer      | M    | R08.1      | cache-store                 |

⚠ `config-types` shared by R04.1, F03.2, P02: serialize or give to one worker.

⚠ `source` shared by F08.2, F08.4: serialize or give to one worker.

### Wave 2

| Task    | Title                                                  | Size | Deps            | Domains            |
| ------- | ------------------------------------------------------ | ---- | --------------- | ------------------ |
| F04     | Exact positional selectors + --task-pattern            | M    | R06, F03.2      | cli, planner       |
| F06.2 ⚠ | Canonical digest function + golden vectors             | M    | F06.1, R03.2    | collector          |
| F08.3 ⚠ | GC honors leases                                       | S    | F08.2           | source             |
| F08.5 ⚠ | Migrate existing contributors to reference API         | M    | F08.4           | source             |
| F10.2   | Tracked contained reads/exists/list with limits        | M    | F10.1           | capability         |
| P03     | omni_plugin contract crate                             | L    | P01.1, P02, R02 | plugin-contract    |
| P10.2   | JS ownership/boundary fixtures                         | M    | P01.3           | docs               |
| P14.1   | Unsupported-feature diagnostic                         | S    | P02             | plugin-config      |
| R03.3 ⚠ | Plumb operands through validation/resolve/display/hash | M    | R03.2           | command, collector |
| R03.4 ⚠ | Mixed-path (C16) prototype and tests                   | S    | R03.2           | command            |
| R04.2   | Layer loader (replace eager load)                      | L    | R04.1           | loader             |
| R08.3   | Migrate task cache onto neutral storage                | M    | R08.2           | cache-store        |

⚠ `command` shared by R03.3, R03.4: serialize or give to one worker.

⚠ `collector` shared by R03.3, F06.2: serialize or give to one worker.

⚠ `source` shared by F08.3, F08.5: serialize or give to one worker.

### Wave 3

| Task    | Title                                            | Size | Deps       | Domains           |
| ------- | ------------------------------------------------ | ---- | ---------- | ----------------- |
| F10.3   | Revalidation (content identity, negative reads)  | M    | F10.2      | capability        |
| P09.1   | Plugin contributor and subsystem selection       | M    | F08.5, P02 | source, cli       |
| P13.1   | WIT package                                      | M    | P03, P01.1 | plugin-wasm       |
| R04.3 ⚠ | Replay and finalize (defaults after composition) | L    | R04.2      | loader, extractor |
| R07.2 ⚠ | Ordered raw layer traversal from extension graph | M    | R04.2      | loader            |

⚠ `loader` shared by R04.3, R07.2: serialize or give to one worker.

### Wave 4

| Task    | Title                                                 | Size | Deps              | Domains               |
| ------- | ----------------------------------------------------- | ---- | ----------------- | --------------------- |
| F05.1   | Origin-tagged cache enablement through CacheInfo      | M    | R04.3             | extractor             |
| F07 ⚠   | Conflict detection and provenance                     | L    | R04.3, R07.2      | loader, config-types  |
| P04.1 ⚠ | Project inference controls                            | M    | R04.3, P02        | config-types          |
| P04.2 ⚠ | Restricted additive plugin contribution types         | M    | R04.3, R03.2, B03 | config-types          |
| P09.2   | Static manifest validation and descriptors            | M    | P09.1             | plugin-config, source |
| R04.4   | Migration fixtures and changelog                      | S    | R04.3             | docs                  |
| R05 ⚠   | Field-wise task composition, atomic alias transitions | M    | R04.3             | config-types          |
| R07.3 ⚠ | Virtual classification in discovery/loader            | M    | R07.1, R04.3      | loader, discovery     |

⚠ `config-types` shared by R05, F07, P04.1, P04.2: serialize or give to one worker.

⚠ `loader` shared by R07.3, F07: serialize or give to one worker.

### Wave 5

| Task    | Title                                                       | Size | Deps                   | Domains                                     |
| ------- | ----------------------------------------------------------- | ---- | ---------------------- | ------------------------------------------- |
| F03.3 ⚠ | Root-reference lowering after composition                   | M    | F03.2, R05             | loader                                      |
| F05.2 ⚠ | Explicit-contradiction error and deciding_source            | M    | F05.1, B04             | executor                                    |
| F06.3 ⚠ | Plumb digest through all collector constructors             | L    | F06.2, R05, B01, F05.1 | extractor, collector, cache-store, executor |
| P03.2   | Wire restricted contribution types into ProjectContribution | S    | P03, P04.2             | plugin-contract                             |
| P04.4 ⚠ | Activation defaults                                         | M    | F05.1, P04.2           | extractor, config-types                     |
| P05 ⚠   | Marker indexing and root classification                     | L    | R07.3, F01, P02        | discovery, loader                           |
| P09.3   | Captured-byte artifact loading                              | M    | F10.3, F08.3, P09.2    | source                                      |

⚠ `loader` shared by F03.3, P05: serialize or give to one worker.

⚠ `executor` shared by F05.2, F06.3: serialize or give to one worker.

⚠ `extractor` shared by F06.3, P04.4: serialize or give to one worker.

### Wave 6

| Task    | Title                                              | Size | Deps                     | Domains         |
| ------- | -------------------------------------------------- | ---- | ------------------------ | --------------- |
| F09     | Eligibility/provenance inspection output           | M    | B04, F05.2               | cli             |
| P04.3   | Activation apply with scoped patches               | L    | P04.1, R05, F03.3, R07.2 | loader          |
| P06.1 ⚠ | Activation graph and binding validation            | M    | P03, F05.2               | plugin-host     |
| P06.2 ⚠ | Stage engine, snapshots, partitions, atomic commit | L    | P03.2, P05, F05.2        | plugin-host     |
| P10.1 ⚠ | JS adapter scaffold + registration                 | S    | P03, F05.2               | plugin-builtins |
| P11.1 ⚠ | Cargo package/aggregate classification             | M    | P03, P01.4, F05.2        | plugin-builtins |

⚠ `plugin-host` shared by P06.1, P06.2: serialize or give to one worker.

⚠ `plugin-builtins` shared by P10.1, P11.1: serialize or give to one worker.

### Wave 7

| Task    | Title                                       | Size | Deps              | Domains         |
| ------- | ------------------------------------------- | ---- | ----------------- | --------------- |
| P04.5   | Activation composition under conflict rules | M    | P04.3, P04.4, F07 | loader          |
| P06.3 ⚠ | Dispositions and transition table           | M    | P06.2             | plugin-host     |
| P06.4 ⚠ | Dependency-ordered inference                | M    | P06.1, P06.2      | plugin-host     |
| P06.5 ⚠ | Claim and query service                     | L    | P06.2, P01.1      | plugin-host     |
| P06.7 ⚠ | Budgets, cancellation, overflow splitting   | L    | P06.2, P01.5      | plugin-host     |
| P10.3 ⚠ | JS manager selection                        | M    | P10.1, P10.2      | plugin-builtins |
| P10.6 ⚠ | JS script tasks                             | M    | P10.1, R03.3      | plugin-builtins |
| P11.4 ⚠ | Cargo build/check/test tasks                | M    | P11.1, R03.3      | plugin-builtins |

⚠ `plugin-host` shared by P06.3, P06.4, P06.5, P06.7: serialize or give to one worker.

⚠ `plugin-builtins` shared by P10.3, P10.6, P11.4: serialize or give to one worker.

### Wave 8

| Task    | Title                                           | Size | Deps                | Domains                  |
| ------- | ----------------------------------------------- | ---- | ------------------- | ------------------------ |
| P06.6 ⚠ | Resolution-only participation and observe       | M    | P06.3, P06.5        | plugin-host              |
| P07 ⚠   | Inference cache                                 | L    | R08.3, P06.3, F10.3 | cache-store, plugin-host |
| P10.4 ⚠ | JS naming and identity/membership claims        | M    | P10.1, P06.5        | plugin-builtins          |
| P11.2 ⚠ | Cargo claims                                    | M    | P11.1, P06.5        | plugin-builtins          |
| P13.3   | Discovery adapter (async host imports, pooling) | L    | P13.1, P13.2, P06.7 | plugin-wasm              |

⚠ `plugin-host` shared by P06.6, P07: serialize or give to one worker.

⚠ `plugin-builtins` shared by P10.4, P11.2: serialize or give to one worker.

### Wave 9

| Task    | Title                                       | Size | Deps              | Domains         |
| ------- | ------------------------------------------- | ---- | ----------------- | --------------- |
| P08.1   | Pipeline wiring into Context                | L    | P04.5, P06.6, P05 | loader          |
| P10.5 ⚠ | JS project edges and dependency claims      | L    | P10.4             | plugin-builtins |
| P11.3 ⚠ | Cargo project edges                         | M    | P11.2             | plugin-builtins |
| P13.4 ⚠ | Limits and containment tests                | M    | P13.3             | plugin-wasm     |
| P13.5 ⚠ | Import-set conformance and pinned toolchain | M    | P13.3             | plugin-wasm, ci |
| P13.6 ⚠ | Artifact loading from captured bytes        | M    | P13.3, P09.3      | plugin-wasm     |
| P14.2 ⚠ | Feature gating and CI matrix                | M    | P14.1, P13.3      | ci              |

⚠ `plugin-builtins` shared by P10.5, P11.3: serialize or give to one worker.

⚠ `plugin-wasm` shared by P13.4, P13.5, P13.6: serialize or give to one worker.

⚠ `ci` shared by P13.5, P14.2: serialize or give to one worker.

### Wave 10

| Task  | Title                                                                                           | Size | Deps         | Domains   |
| ----- | ----------------------------------------------------------------------------------------------- | ---- | ------------ | --------- |
| P08.2 | Bootstrap capture and final revalidation                                                        | M    | F10.3, P08.1 | loader    |
| P08.3 | Inferred-task identity and cache fallback wiring                                                | M    | F06.3, P08.1 | extractor |
| P08.5 | Impact-relation selection over retained claims (cycle-safe, transitive, excluded-root fallback) | L    | P08.1, P06.5 | planner   |

### Wave 11

| Task  | Title                                             | Size | Deps       | Domains             |
| ----- | ------------------------------------------------- | ---- | ---------- | ------------------- |
| P08.4 | Publication gating and rejected-pass invisibility | M    | P07, P08.2 | loader, plugin-host |

### Wave 12

| Task    | Title                               | Size | Deps                              | Domains         |
| ------- | ----------------------------------- | ---- | --------------------------------- | --------------- |
| P10.7 ⚠ | JS end-to-end conformance           | M    | P10.3, P10.5, P10.6, P08.4, P08.5 | plugin-builtins |
| P11.5 ⚠ | Cargo end-to-end conformance        | M    | P11.3, P11.4, P08.4, P08.5        | plugin-builtins |
| P12     | Plugin inspection, notices, CLI/MCP | M    | P08.4, F09                        | cli             |

⚠ `plugin-builtins` shared by P10.7, P11.5: serialize or give to one worker.

### Wave 13

| Task  | Title                       | Size | Deps                              | Domains     |
| ----- | --------------------------- | ---- | --------------------------------- | ----------- |
| P13.7 | Native/Wasm parity fixtures | M    | P13.4, P13.5, P13.6, P10.7, P11.5 | plugin-wasm |

### Wave 14

| Task | Title                                             | Size | Deps                       | Domains |
| ---- | ------------------------------------------------- | ---- | -------------------------- | ------- |
| P15  | Conformance suite, benchmarks, budget calibration | L    | P10.7, P11.5, P13.7, P14.2 | ci      |

### Wave 15

| Task | Title                                    | Size | Deps | Domains  |
| ---- | ---------------------------------------- | ---- | ---- | -------- |
| P16  | Stabilization, compatibility, migrations | L    | P15  | docs, ci |

<!-- END GENERATED -->
