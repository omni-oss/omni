# RFC 0024: Tiered, hook-based project discovery plugins

- **Status:** Draft <!-- Draft | Accepted | Implemented | Superseded -->
- **Owner:** TBD
- **Date:** 2026-11-05
- **Base branch:** `main`
- **Affected crates/files:** `omni_configurations` (workspace activation, plugin manifests, project inference controls and presence-aware layers), `omni_context` (discovery, loading, provenance and extension resolution), `omni_configuration_discovery`, `config_utils`, `omni_remote_source_contributors`; proposed `omni_plugin` contract and native/Wasm/JavaScript adapters; CLI and MCP inspection surfaces; schemas and discovery benchmarks
- **Related:** RFC 0011 (glob configuration), RFC 0013 (remote sources), RFC 0014 (packs), RFC 0021–0023 (diagnostics, redaction, notices and tracing)

## 1. Summary

Allow explicitly enabled plugins to infer projects from ecosystem manifests,
without requiring a `project.omni.yaml` in every directory. The initial native
plugins are `@omni/javascript`, reading `package.json`, and `@omni/rust`, reading
Cargo manifests (`Cargo.toml`).
The result is ordinary omni project configuration, not a second project model.

One capability contract has three implementation tiers:

| Tier | Delivery | Execution |
| --- | --- | --- |
| Native | Compiled into omni | Reviewed Rust implementation using the host's bounded discovery services |
| Wasm | User-authored, pinned artifact | Sandboxed runtime with metering and resource limits |
| JavaScript | User-authored, pinned module bundle | Restricted, interruptible embedded runtime; not Node.js |

Availability is not activation. **All tiers are disabled unless explicitly
enabled in workspace configuration.** Finding a manifest, installing a pack, or
shipping a new omni binary never enables a plugin.

An explicit project file is authoritative. It can extend inferred configuration
or replace it completely. The recommendation is **extend by default**, with a
per-project `inference: {mode: replace}` escape hatch that skips plugin inference
for that root.

Use the term **plugin**: an executable participant in a versioned host protocol.
Keep **pack** for declarative distribution and composition. Start with project
inference only; do not introduce a competing generator/tool registry.

All syntax and API names below are proposals, not existing functionality.

## 2. Motivation and current behavior

Today, `WorkspaceConfiguration.projects` selects directories through
`GlobConfig`. `omni_context::ProjectDiscovery` searches those directories for
`project.omni.{yaml,yml,json,toml}` using the existing ignore-aware walker.
`Context::load_discovered_project_configs` loads real files, resolves project
extensions, then resolves task extensions.

`DiscoveredPath::Virtual { dir }` already exists, but is unused and filtered out
by the loader. It is a useful seam, not a complete inferred-project mechanism:
loading, identity, provenance, hashing and inspection still assume file-backed
configuration in several places.

This makes onboarding repetitive. Ecosystem manifests already describe names,
scripts and local dependencies, but users must translate those into omni files.
Inference should remove that duplication without making behavior surprising,
weakening explicit configuration, or making every command run repository code.

Packs already register generators, tools and projections (RFC 0014). Plugins
should solve the missing executable inference problem rather than duplicate that
distribution system.

## 3. Goals / Non-goals

**Goals**

- Discover useful projects with no per-project omni file, inside an explicitly
  configured omni workspace and its project selection boundaries.
- Offer native speed and user-authored Wasm/JavaScript implementations through
  the same deterministic, versioned data contract.
- Make activation, inferred contributions, precedence and costs inspectable.
- Preserve explicit configuration as the final authority, including complete
  replacement and clearing of inferred collections.
- Bound work, avoid repeated scans and interpreter startup, and cache safely.
- Keep CLI and MCP semantics identical.

**Non-goals**

- Workspace auto-creation, implicit activation based on installed dependencies,
  or running package lifecycle scripts during discovery.
- External native dynamic libraries, arbitrary hooks into omni internals,
  task executors, or a general-purpose plugin marketplace.
- Letting discovery write files, spawn processes, access the network, or alter
  workspace security policy.
- Generator/tool injection in the initial protocol, or claiming that arbitrary
  native code can be safely preempted in-process.

## 4. Naming and scope

### 4.1 Plugin, not extension

Recommend `plugin`, `plugins:` and `plugin.omni.*`. The name describes code
called through a constrained host API and is familiar for ecosystem adapters.
“Extension” is broader and already overlaps with configuration `extends` and
omni's extension graph. “Discovery provider” accurately names the initial
capability, but is too narrow for the distribution unit if capabilities grow.

An implementation tier is not a permission tier. Native plugins do not receive
extra semantic authority, and JavaScript plugins do not receive fewer project
fields merely because they use a different language.

### 4.2 What plugins can do

| Capability | Recommendation | Reason |
| --- | --- | --- |
| Find project roots from ecosystem markers | Initial capability | Main missing feature |
| Infer names, metadata, tasks and local dependency edges | Initial capability | Makes discovery useful without handwritten omni files |
| Infer task input/output/cache configuration | Allowed, conservatively | Must be deterministic; unsafe cache assumptions are worse than no cache |
| Register generator/tool/projection sources dynamically | Not initially | Duplicates packs and makes discovery mutate global registries |
| Execute generators or tools inside the plugin runtime | Not initially | Different lifecycle, permissions and performance model |
| Arbitrary graph/execution hooks | Out of scope | Unbounded effects and ordering complexity |

Inferring a project's `tasks.build` is not registering a workspace tool. A
plugin can describe an ordinary command, but discovery never executes it. Such
tasks run only through omni's normal task selection and execution policies;
plugin activation is not permission to run inferred tasks automatically.

For ecosystem kits, recommend a repository containing a plugin artifact and a
normal pack containing generators/tools/projections. Users register both
explicitly. A future RFC may let packs *distribute* plugin declarations, but
distribution must still not activate code transitively. If dynamic contributions
prove necessary, add a separately enabled, versioned capability outside project
discovery, using existing registries and collision rules.

### 4.3 Initial native ecosystem plugins

Name builtins consistently by ecosystem, not by a mix of language and tool:

- **`@omni/javascript`** initially discovers JavaScript/TypeScript projects from
  `package.json`, inferring package names, scripts, workspace membership and
  local dependency edges. npm, Yarn, pnpm and Bun are supported package-manager
  choices within this plugin, not separate discovery plugins. Package-manager
  detection selects task commands without running installs or scripts during
  discovery; document detection precedence and ambiguity handling in the adapter
  specification before implementation.
- **`@omni/rust`** initially discovers Cargo projects from `Cargo.toml`, including
  workspace membership and local dependency edges, and infers ordinary Cargo
  task definitions. It is not a promise to support every Rust build system or
  every Cargo feature in the first release.

Both are opt-in native handlers using `project_discovery` and the same lifecycle
and host services as external plugins. Ecosystem names leave room for future
manifest support without renaming; they do not imply that additional formats
are supported today. Keep detailed ecosystem inference rules and fixtures with
each adapter, separate from the generic contract.

## 5. Activation, identity and delivery

Proposed `workspace.omni.yaml`:

```yaml
projects:
  include: ["crates/**", "packages/**"]
  exclude: ["**/fixtures/**"]

plugins:
  - id: javascript
    enabled: true
    source: builtin
    name: "@omni/javascript"
    options: {}

  - id: rust
    enabled: true
    source: builtin
    name: "@omni/rust"
    options: {}

  - id: company
    enabled: true
    source: local
    path: ./omni/plugins/company

  - id: frontend
    enabled: false
    source: git
    uri: https://github.com/example/omni-frontend.git
    rev: <immutable-commit>
    subdir: plugins/frontend # optional literal directory; default is repo root
```

- `plugins` defaults to `[]`; each entry's `enabled` defaults to `false`.
  Disabled entries are not fetched, loaded or executed. Structural declaration
  errors are still reported without reading their source.
- `id` is the consumer handle, unique within the workspace. External manifests
  declare canonical `name` and metadata `version`, following RFC 0014's
  consumer-id/author-name distinction.
- `builtin` resolves a compiled registry entry. External `local`/`git` entries
  resolve one `plugin.omni.*` manifest at the selected artifact directory using
  existing source materialization and immutable lock identity (RFC 0013), not
  a separate downloader. Local `path` is one literal directory; Git `subdir`
  defaults to the checkout root. Neither accepts discovery globs.
- Loading an enabled plugin with no mutually supported protocol version or an
  unsupported runtime is an error. There is no silent fallback to another tier.
- A built-in's implementation identity includes the omni build. An external
  plugin's identity includes the verified artifact and bundled module digests;
  a version string alone is never a cache or trust identity.

An external manifest might be:

```yaml
name: "@example/company"
version: 0.1.0
protocol: ">=0.1.0, <0.2.0"
runtime: wasm # alternatively javascript
entry: ./dist/plugin.wasm
capabilities:
  - name: project_discovery
    api: ">=0.1.0, <0.2.0"
    required: true
    requires_features: []
    hooks: [on_workspace, filter_candidates, infer_roots, resolve_dependencies]
discovery:
  markers: [company.project.json]
  inputs: [company.project.json, company.workspace.json]
```

Marker and input declarations are static data readable without starting the
runtime. JavaScript entries are self-contained bundles (or an explicitly listed,
hashed local module graph); no npm resolution, network imports, install scripts
or ambient Node installation. Source and entry paths cannot escape the
selected artifact directory, not merely the surrounding repository. Git source
availability, lazy materialization, pinning and offline behavior follow §15's
shared-source integration. No registry is required for v1.

Capability names are canonical protocol identifiers; `project_discovery` is the
initial namespace. Each capability has an API requirement independent of the
plugin envelope's protocol requirement. Both use explicit semver ranges;
experimental examples deliberately constrain the minor version. `required`
defaults to `true`. An unknown/incompatible required capability or missing
required feature fails activation; an explicitly optional capability can instead
be skipped with a notice and inspection entry. Unknown hooks within a negotiated
capability version are errors, not silently ignored. Hook declarations are
validated against the handler's supported-hook set before scheduling. See §14
for negotiation, stabilization and migration guarantees.

## 6. Discovery and the host contract

### 6.1 Hook namespaces and lifecycle

Plugins are **stateless but not sessionless**: they do not retain their own
memory between calls, and they never own mutable state. Instead, omni owns a
per-plugin, per-capability, per-discovery **session state** that plugins populate
and receive back in later hooks. This lets plugins be interrupted, retried,
cached and scheduled freely while still accumulating context across a pass.

The contract is a set of **stage hooks**. Every hook is **namespaced by
capability**, so adding a new capability later cannot collide with discovery
hooks or with hooks from another capability. A hook name has the shape
`<capability>.<stage>` (for example `project_discovery.infer_roots`). Plugins
register separate typed capability handlers. Each capability owns a fixed
namespace, version and hook vocabulary; there is no single `capability_id()`
method on a multi-capability plugin. The host invokes only explicitly supported
hooks of negotiated capabilities. Unsupported required capabilities fail;
explicitly optional ones follow §5's skip policy.

The initial capability is `project_discovery`. Its stages (plugins implement
only those they need) are:

1. **`project_discovery.on_workspace`** — receive workspace options and static
   discovery scopes. May return a small, serializable workspace state seed.
   No file-content access.
2. **`project_discovery.filter_candidates`** — receive candidate roots and a
   bounded file index; may decline roots or request additional declared inputs.
   Runs before any heavy parsing.
3. **`project_discovery.infer_roots`** — batched per plugin over multiple
   roots; returns project fragments, local dependency references, and an
   updated session state. This is the only hook that may parse declared input
   files.
4. **`project_discovery.resolve_dependencies`** — runs after all plugins'
   `infer_roots` complete; receives the accumulated session state plus the full
   set of inferred roots, allowing cross-root and cross-plugin resolution
   without guessing final omni names.
5. **`project_discovery.observe`** — optionally inspect the frozen, composed
   project/dependency snapshot and return notices or inspection metadata only.
   It cannot change roots, fields or edges. All fragment edits finish before
   dependency resolution; there is no mutating post-resolution finalizer.

Future capabilities (for example `generator_contribution` or
`tool_contribution`) would define their own namespace and stages; the host
schedules each capability's stages independently and never assumes a flat
global hook list. Session state is also partitioned by capability namespace,
so one capability's state cannot be read by another.

The host may:

- batch many roots into one `infer_roots` call instead of one call per
  directory;
- run independent plugins concurrently;
- defer or skip unsupported hooks;
- cache and reuse session state fragments keyed by inputs;
- interrupt and retry a hook without leaking plugin memory, because the plugin
  holds no state of its own.

Plugins cannot assume invocation order beyond the stage order, and cannot assume
they are the only plugin. They must tolerate re-invocation and must not depend
on wall-clock or randomness.

#### Session partitions and stage transactions

Session state has two partitions: **workspace state**, written only by the
singleton `on_workspace` hook, and **root state**, keyed by stable candidate
root. Later hooks receive workspace state read-only. Root-oriented stages may
replace state only for their assigned roots; cross-root knowledge is supplied
through immutable stage snapshots, not shared writes.

Every invocation within a stage reads the same committed stage-start snapshot.
Omni assigns each root to at most one invocation in that stage. Batches may run
in parallel, but cannot overwrite another batch's root state. Cross-root
dependency resolution reads the complete frozen inference snapshot and emits
only results for assigned source roots. A result for a root must be independent
of its batch neighbors and invocation order.

Omni validates all invocations before atomically committing that stage's state,
contributions and notices. A failed or cancelled stage commits nothing; retries
start from its original snapshot. There is no last-writer-wins merge and no
plugin-defined state reducer. Workspace data discovered during inference can
be represented as root-owned facts and queried in the next stage's snapshot,
without introducing a mutable workspace accumulator.

Observation has no state writes. Capability handlers declare a supported-hook
set; typed methods have identity/no-op defaults for omitted hooks. SDK/native
registration validation rejects declarations without implementations and
implementations without declarations. The host never calls an omitted hook.

### 6.2 Pipeline

1. Load workspace config and validate explicit activation. With no enabled
   plugins, retain the existing discovery path without runtime initialization.
2. Resolve enabled plugin descriptors. Compile their marker/input globs with
   the shared glob model, not one glob implementation per runtime.
3. Use one host-owned, ignore-aware walk to index explicit project files and
   enabled markers. Keep `projects` includes/excludes, ignore files, cache
   exclusions and symlink containment identical across tiers. No plugin may
   expand discovery outside that boundary.
4. Read explicit-file inference controls before scheduling plugin work. Roots
   marked `replace` need no inferred contribution. Batch remaining candidate
   roots by plugin; do not invoke an interpreter per directory.
5. Load the explicit extension closure as presence-aware layers, preserving each
   layer's path origin. Run inference hooks to produce fragments; freeze them
   before plugin dependency-resolution hooks read the complete snapshot.
   Resolution hooks return root-based edges, not edits to project fields.
6. Compose inferred contributions beneath explicit base/concrete layers (§7),
   apply defaults, and resolve task extensions. Resolve root-based edges to
   final explicit project names, then run configuration/graph validation.
7. Invoke optional read-only observation hooks on the final frozen snapshot.
   Return one project model for all downstream consumers.

Multiple markers at one canonical root produce one project, not duplicate
projects. Nested roots are independent if selected; detecting a parent does not
implicitly claim or suppress its children. Ecosystem workspace membership can
help interpretation but cannot bypass omni's selection boundary.

Workspace-level manifests may be declared read-only inference inputs without
making their directories projects. Return no project for aggregate-only
manifests unless the ecosystem adapter can justify a runnable root project.

### 6.3 Data boundary

Define a versioned, serializable contract in a small shared crate. Native plugins
use the same logical request/response as external ones without requiring an
unnecessary serialization round trip.

Requests contain candidate root ids, a bounded file index, options and a
read-only host file service. Responses contain project fragments keyed by root,
local dependency references, provenance, structured non-fatal notices, and an
stage-appropriate state replacements. Represent local dependencies by stable workspace-relative
roots where possible, not guessed final omni names: explicit configuration may
rename a project. Non-local package dependencies must not become spurious omni
project edges.

Host-managed configuration fields (`file`, `dir`, inference controls and
activation/security policy) cannot be emitted by plugins. Plugin output cannot
introduce configuration `extends` paths; ecosystem inheritance must be handled
through declared, tracked input reads. Unknown fields and invalid paths fail
validation. A plugin may decline a candidate, but cannot return arbitrary roots
that were never in its host-approved candidate set.

The file service validates canonical containment, records successful and missing
reads, and applies byte/read limits before allocation. Reads must match declared
input scopes. Additional inputs discovered from a manifest are allowed only
within those scopes and are tracked. No raw host filesystem handle, environment,
wall clock, randomness, network or subprocess service is exposed.

Session state is owned by omni: it is serializable, size-bounded and versioned.
Hook cache keys include incoming state fingerprints; cache entries contain both
outputs and state replacements with their tracked input dependencies. State may
contain parsed facts derived from tracked inputs, but never bypass input
tracking. Partition keys include workspace/session, plugin activation id,
artifact identity, capability/version and (for root state) root id. Native
handlers may use typed in-memory state without serialization on every call;
external adapters and persistent caches use the versioned serialized form.

Virtual projects need a stable logical configuration identity distinct from a
physical filename. Preserve root and plugin/artifact provenance rather than
pretending a `project.omni.yaml` exists. Update extension graph ids, config
inspection and cache consumers accordingly; merely accepting the existing
`Virtual` enum variant is insufficient.

## 7. Explicit configuration and composition

### 7.1 Project modes

No explicit file: the merged inferred layer is the project. An explicit file
defaults to `extend` and can control participation:

```yaml
# packages/web/project.omni.yaml
inference:
  mode: extend
  plugins: [frontend] # optional allowlist of already enabled workspace ids
name: web
tasks:
  build:
    exec: pnpm run build:production
```

```yaml
# Fully handwritten: none of this root's inferred fields survive.
inference:
  mode: replace
name: web
tasks:
  build:
    exec: ./build.sh
```

`replace` bypasses all inference for this root, including plugin conflicts and
invalid inferred output. Explicit `extends` continues to work: it replaces
*inference*, not ordinary explicit inheritance. The `plugins` allowlist only
narrows activation and is invalid with `replace`; it cannot enable anything.
These controls belong to the concrete discovered file and are not inherited
from project bases. Explicit `base: true` continues to exclude a template from
the runnable project set, including when an ecosystem marker shares its root.

### 7.2 Layer order and merge semantics

Low to high precedence:

1. Selected plugin fragments for this root.
2. Explicit project's resolved base-file layers, in existing extension order.
3. The concrete explicit project's own fields.

Project inference is applied once at the discovered root, not recursively at
every `extends` target. Resolve task extensions after project-layer composition,
as today. An extension target is not activated as a runnable inferred project
merely because another project references its file.

**Preserve field presence until composition finishes.** Do not deserialize a
partial explicit file into today's default-filled `ProjectConfiguration` and
accidentally erase inference with missing fields. Introduce a typed project
layer/patch representation recursively, including task/cache/environment layers,
compose it, then apply defaults and validate the
final `ProjectConfiguration`. Extend mode may omit `name` when inference
provides it; replace mode must obtain a name from explicit configuration or its
explicit bases, as normal. This is a deliberate schema/loader change.

Reuse `config_utils` semantics for collections, including explicit `merge`,
`append`, `prepend` and `replace` where supported. For example, discard all
inferred dependencies or tasks without disabling other inference:

```yaml
dependencies:
  replace: []
tasks:
  replace:
    build:
      exec: ./build.sh
```

Explicit scalar values win; absent values inherit. Do not invent a generic
`null` deletion rule or silently change existing merge behavior. In extend
mode, overriding only `tasks.build.exec` retains other inferred build fields;
use the task dictionary's `replace` form or project replacement when that is
undesired. Fine-grained removal of a single inferred task is deferred unless
the shared config model gains that operation for all configuration sources.

### 7.3 Multiple plugins and conflicts

Do not let scheduling, runtime tier or activation-list order decide winners.
Disjoint contributions compose; identical values coalesce. Different values
for the same scalar or task field are a conflict unless a higher explicit layer
replaces that field/subtree. List contributions use a documented stable plugin
id order and the shared collection semantics, with dependencies normalized to
unique resolved edges. Changing ids can therefore change ordered-list output
and invalidates the inference cache.

Report unresolved conflicts with both plugin ids, root and field path. Users
can resolve them in the explicit file, select contributors with its allowlist,
or choose replacement. Duplicate final project names across distinct roots
remain errors; never silently choose a project. Malformed output is an error,
not a merge conflict that an unrelated explicit field can mask.

## 8. Performance is a contract

“Must not tank performance” cannot mean that arbitrary code costs nothing.
It means disabled plugins cost effectively nothing, unchanged work is reused,
and enabled work has measurable limits rather than unbounded startup cost.

### 8.1 Shared safeguards

- One shared walk and file-read cache; no full repository traversal per plugin.
- Static marker filtering before loading a runtime. Batch requests and lazy
  runtime initialization; reuse an instance within a command or MCP process,
  without retaining state that affects inference results.
- A bounded worker pool, cancellation propagation and host-wide limits in
  addition to per-plugin limits. Many small invocations must not multiply a
  nominal per-call budget into an unlimited discovery phase.
- Bound file reads, bytes, candidate count, output bytes/project/task count,
  recursion and notices. Validate output before expanding it into graph state.
- Cache inferred fragments separately from final explicitly composed projects.
  Key by host protocol/build, plugin artifact/module closure, options,
  contributor ids and declared-input fingerprints. Track missing files and
  scoped directory membership so creation/deletion also invalidates results.
  Input scope declarations are part of the key; no untracked reads are allowed.
- Include explicit files, extension closure and normalized inferred config in
  effective configuration/task cache identity. A plugin task change must not
  reuse an old execution cache entry.
- No background inference for commands that do not need projects. Long-lived
  MCP state must invalidate on input, option and activation changes, including
  disabling a plugin or editing a local bundle.
- Reuse a cached hook result only when its request, incoming state and all
  observed inputs/snapshots match. Cached state alone does not justify skipping
  a hook: cross-root resolution also depends on the frozen root snapshot.

### 8.2 Tier-specific enforcement

| Tier | Required controls | Honest limitation |
| --- | --- | --- |
| Native | Host read/output budgets, bounded algorithms, cooperative cancellation, reviewed implementations and regression benchmarks | A timeout around an async call cannot stop an infinite Rust loop or cap arbitrary native allocation |
| Wasm | Fuel/epoch interruption, wall deadline, linear-memory/stack limits and bounded host calls; no ambient WASI access | Host calls must also be cancellable and bounded |
| JavaScript | Interrupt/termination hook, heap/stack limits, deadline and bounded host calls; no Node APIs | A promise timeout without VM interruption is not isolation |

Native plugins are trusted omni code, not third-party native loading. Never
claim external-grade isolation for them. If hard preemption of native plugins
is required, use a killable worker process; do not detach timed-out threads and
let them continue consuming CPU. A native implementation that cannot remain
bounded is not eligible for the in-process tier.

Initial benchmark budgets (proposed, to calibrate before acceptance):

- No enabled plugins: no VM startup, no artifact fetch, and at most 2% median
  discovery-time regression against baseline on the same fixtures/hardware.
- Warm unchanged inference: no plugin execution and at most 10% median overhead
  over loading equivalent explicit projects on a 1,000-project fixture.
- Default external plugin envelope: 64 MiB runtime-managed memory, 16 MiB read
  bytes and 8 MiB output per batch, 1 second execution deadline per batch, plus
  a 5 second aggregate inference execution deadline per command. Fuel and
  candidate limits must be finite and calibrated alongside these numbers.
- Bound queued/in-flight batches and aggregate memory across runtimes. Report
  source materialization and discovery I/O separately; execution deadlines are
  not a promise that a large cold checkout finishes within five seconds.

These are release gates/default proposals, not measured guarantees. Keep limits
finite and workspace-owned; plugins cannot raise them. Benchmark cold native,
Wasm and JavaScript separately, including runtime startup, and publish median,
p95, allocations/peak memory, reads and cache hits. Compare fixtures with
equivalent results, not a plugin-enabled graph against an empty baseline.

## 9. Failures, security and observability

Enabled-plugin failures are fatal by default: timeout, resource exhaustion,
unsupported required protocol/capability, invalid output and inference errors must not quietly
produce an incomplete graph. Commit no partial response from a failed batch;
do not silently reuse stale output. No automatic “disable the slow plugin and
continue” policy: command meaning must not depend on machine speed.

Replacement roots remain usable without executing their inference, but cannot
magically recover a command that still needs other roots from a failing plugin.
Users can explicitly disable a plugin and provide handwritten configurations.

Use RFC 0021 diagnostics for fatal failures and RFC 0023 notices for successful
but degraded interpretation (for example, an ecosystem feature deliberately not
inferred). Present bounded, sanitized plugin text; provenance never exempts
output from RFC 0022 redaction. Developer timings, cache reasons and host-call
counts go to tracing, not routine terminal noise. Hook notices are returned or
buffered transactionally alongside contributions and state, never published
immediately through a reporter. Failed attempts publish nothing; cached or
retried successful results are committed once per stage in deterministic order.
Immediate developer tracing is allowed and carries invocation/attempt ids.

Propose CLI and MCP inspection operations for:

- Available/enabled plugins, runtime, canonical identity, artifact digest,
  declared scopes and effective limits.
- Effective project configuration with field-level plugin/file provenance,
  skipped/replaced contributors and merge decisions.
- Discovery cost breakdown and inference-cache status.

Exact command names are an implementation detail; both surfaces must expose
structured results rather than requiring users to parse logs. Inference never
writes `project.omni.yaml` automatically. A future explicit export command can
materialize configuration for migration or debugging.

Sandboxing is mandatory for external discovery plugins regardless of the
existing experimental generator-capabilities switch. It must not inherit that
switch's unconfined fallback. No permitted discovery API grants access outside
approved workspace inputs or artifact modules. Repository-controlled files are
untrusted data, not executable configuration.

## 10. Implementation and rollout

1. **Contract and composition first:** typed layers, inference controls,
   virtual identities, conflict handling, provenance, final-name dependency
   resolution, hook stage definitions, session-state serialization and
   zero-plugin regression tests. No runtime dependency needed.
2. **Native pilots:** opt-in `@omni/javascript` and `@omni/rust` handlers behind
   the same protocol and hook stages, host-owned candidate scan and input
   tracking. Specify package-manager detection and ecosystem inference rules
   in separate adapter documentation; initial scope is §4.3.
3. **Wasm:** versioned SDK/fixtures, pinned source loading, sandbox, resource
   enforcement and cache invalidation. No external runtime ships without its
   cancellation and containment tests.
4. **JavaScript:** the same conformance suite and SDK data model, implemented
   with a runtime selected by startup/memory/interruptibility benchmarks.
5. **Stabilize:** publish baseline measurements and freeze protocol/capability
   1.0 no later than omni 1.0, with §14's compatibility/migration tests and
   tooling. Retain explicit activation for every tier after stabilization.

Do not select a Wasm or JavaScript engine solely in this proposal. Evaluate
binary size, cold start, target-platform support, deterministic imports, license
and enforceable interruption before committing dependencies. Missing-runtime
builds reject activation clearly rather than invoking a system interpreter.

Required tests include:

- Native, Wasm and JavaScript contract fixtures produce identical normalized
  output, provenance and validation outcomes for equivalent implementations.
- Hook ordering and batching do not change normalized results.
- Session state round-trips and cache invalidation work across hook
  re-invocation.
- Parallel batches cannot lose state updates or write another batch's roots;
  failed stages and retries publish neither partial state nor duplicate notices.
- Required/optional capability negotiation and registered-hook validation reject
  misspelled names and inconsistent exports. Multi-capability registrations keep
  state and dispatch isolated.
- Post-resolution observation cannot mutate projects/edges; dependency-hook
  caches invalidate when their frozen cross-root snapshot changes.
- No enabled plugins preserves existing discovery, extension, cache-exclusion
  and ignore behavior; disabled sources are never fetched or instantiated.
- Inferred-only, extend, replace, missing-name, base-template and explicit
  extension cases, including absence versus default-valued fields.
- Explicit renaming preserves root-based dependency edges; unresolved/excluded
  local roots and cycles receive ordinary graph diagnostics.
- Conflicts are deterministic under reordered scheduling; explicit replacements
  and allowlists resolve only the intended contributions.
- Cache invalidation covers edits, new/deleted/missing inputs, directory
  membership, local bundle changes, options, host upgrades and activation.
- Infinite loops, allocation/output/read floods, path traversal, symlink escape,
  forbidden imports and denied host operations fail within the external budget.
- Performance fixtures cover no plugins, unmatched markers, 1,000 projects,
  many enabled plugins, cold/warm caches and long-lived MCP invalidation.

## 11. Alternatives and discussion points

- **Call everything extensions:** broader branding, but less precise and
  confusable with existing inheritance. Recommend plugins.
- **Discovery only returns roots:** smaller API, but still requires every user
  to write project tasks and dependencies. Recommend bounded project inference.
- **Plugins register tools/generators too:** convenient single-author story,
  but duplicates packs and couples cheap discovery to global registry mutation.
  Recommend colocated plugin + pack now; revisit a separate capability only
  with concrete cases static packs cannot express.
- **Explicit files replace inference by default:** strongest isolation from
  inference changes and worth discussing. Recommend extend for gradual adoption,
  with an obvious complete-replacement switch and field-level inspection.
- **Activation-list order decides precedence:** simple, but reordering unrelated
  entries can change execution. Recommend conflict detection and explicit wins.
- **Only native or only Wasm:** native alone blocks user-authored adapters;
  Wasm alone imposes authoring/startup costs on built-ins. One contract and
  staged runtime adapters retain both advantages. JavaScript is an ergonomic
  option, not a requirement to embed Node.
- **Hard native deadlines:** require process isolation, with IPC/startup cost.
  Recommend reviewed cooperative native code initially; revisit worker-process
  isolation if benchmarks or the trust model require a hard resource boundary.
- **One-shot fragment return instead of hooks:** simpler API, but forces every
  plugin to do all work in one call and prevents the host from batching,
  deferring cross-root resolution, or reusing partial state. Recommend the
  hook model with omni-owned session state.

Before acceptance, settle benchmark-calibrated limits and runtime engines. The
initial builtin set is JavaScript and Rust (§4.3). Extend remains the chosen
default, with explicit full replacement. None of those decisions should weaken activation, explicit authority,
deterministic output or the ban on unbounded external execution.

## 12. Rust trait sketches (proposal)

These are illustrative, not final APIs. Native plugins implement capability
traits; external runtimes adapt the same logical calls over the wire. Hooks are
namespaced by capability, so capability traits are separate and independently
versioned. Plugins are stateless; all mutable progress lives in host-owned
workspace/root state partitions. Supporting request/result types are abbreviated
below; these sketches describe ownership and dispatch, not a complete SDK.

### 12.1 Identity and capability registration

`omni_plugin_configurations` owns manifest/configuration types. A capability
declaration contains its canonical name, API version range, required/optional
flag, required features and supported hooks. The manifest also declares its
supported protocol range. These are data, not executable handlers.

The consumer activation id belongs to the host binding, not the author handler:
the same artifact may be enabled with different ids/options. Native plugins
register typed handlers using a host registry:

```rust
pub trait Plugin: Send + Sync {
    fn manifest(&self) -> &PluginManifest;
    fn register(&self, registry: &mut CapabilityRegistry)
        -> Result<(), PluginError>;
}

impl CapabilityRegistry {
    pub fn project_discovery(
        &mut self,
        hooks: ProjectDiscoveryHooks,
        handler: Arc<dyn ProjectDiscoveryCapability>,
    ) -> Result<(), PluginError>;
}
```

The registration method fixes namespace `project_discovery` and declares handler
versions/features; it cannot register a discovery handler under an unrelated name. Future
capabilities add typed registration entries. Duplicate registrations, unknown
hooks, manifest/handler disagreement and hook implementation mismatches fail
validation. SDK registration helpers/derive generate the supported-hook set from
explicitly annotated implementations; do not attempt runtime reflection over
Rust trait overrides. External modules provide the equivalent validated export
table. One plugin can register several independent capability handlers.

### 12.2 Capability traits (plugin side)

Each capability is its own trait. Adding a capability means adding a trait and a
namespace; existing capabilities are untouched.

```rust
pub trait ProjectDiscoveryCapability: Send + Sync {
    fn on_workspace(
        &self,
        ctx: &WorkspaceContext<'_>,
        req: OnWorkspaceRequest,
    ) -> Result<WorkspaceOutput, PluginError> {
        Ok(WorkspaceOutput::unchanged())
    }

    fn filter_candidates(
        &self,
        ctx: &CandidateContext<'_>,
        req: FilterCandidatesRequest,
    ) -> Result<FilterCandidatesResponse, PluginError> {
        Ok(FilterCandidatesResponse::keep_all(req))
    }

    fn infer_roots(
        &self,
        ctx: &InferenceContext<'_>,
        req: InferRootsRequest,
    ) -> Result<InferRootsResponse, PluginError> {
        Ok(InferRootsResponse::unchanged())
    }

    fn resolve_dependencies(
        &self,
        ctx: &ResolutionContext<'_>,
        req: ResolveDependenciesRequest,
    ) -> Result<ResolveDependenciesResponse, PluginError> {
        Ok(ResolveDependenciesResponse::unchanged())
    }

    fn observe(
        &self,
        ctx: &ObservationContext<'_>,
        req: ObserveRequest,
    ) -> Result<ObserveResponse, PluginError> {
        Ok(ObserveResponse::empty())
    }
}
```

Contexts contain read-only state views. Requests/results are distinct types per
stage: inference may return project fragments, resolution may return root edges,
and observation may return notices/inspection metadata only. Defaults preserve
incoming state and produce no contributions; filtering's default keeps all
candidates. Registered hooks are the only methods omni schedules.

### 12.3 Host-owned state

```rust
pub struct StateValue {
    pub version: u32,
    pub bytes: Vec<u8>, // bounded serialized form
}

pub struct StateSnapshot {
    pub workspace: Option<StateValue>,
    pub roots: BTreeMap<ProjectRootId, StateValue>,
}

pub struct RootOutput<T> {
    pub root: ProjectRootId,
    pub contribution: T,
    pub state: StateUpdate, // Keep | Replace(StateValue) | Clear
    pub notices: Vec<PluginNotice>,
}
```

Snapshots are immutable invocation inputs. The host checks root ownership,
budgets and stage-allowed writes, then commits the entire validated stage
transaction. Workspace output can update workspace state only during
`on_workspace`. There is no plugin-visible `StateStore` or mutable shared
handle. Serialized state is an interchange/cache format, not a requirement to
encode/decode native state on every invocation.

### 12.4 Host services (plugin side view)

```rust
pub trait TrackedInputs: Send + Sync {
    fn read(&self, path: &WorkspaceInputPath) -> Result<InputBytes, HostError>;
}

pub struct InferenceContext<'a> {
    pub inputs: &'a dyn TrackedInputs,
    pub state: &'a StateSnapshot,
    pub options: &'a PluginOptions,
    pub limits: &'a PluginLimits,
    pub cancellation: &'a Cancellation,
}
```

Only inference contexts expose file-content reads. Other contexts expose the
appropriate bounded file index, immutable state or frozen project snapshot.
The input service owns containment, declared-scope checks, negative-read
tracking and allocation budgets. It never returns a raw filesystem handle or
absolute canonical paths. No context exposes immediate user-facing publication;
notices are transactional output values.

### 12.5 Runtime adapters

External runtimes implement the same capability traits by serializing requests
and enforcing budgets. The adapter owns the module handle; the plugin itself
holds no state.

```rust
pub trait RuntimeAdapter: Send + Sync {
    fn kind(&self) -> RuntimeKind;
    fn invoke_hook(
        &self,
        module: &ModuleHandle,
        hook: &HookId, // validated namespace + stage + capability version
        request_bytes: &[u8],
        state_bytes: &[u8], // immutable stage-start snapshot
        services: &InvocationServices<'_>, // stage-scoped, tracked host imports
        limits: &PluginLimits,
    ) -> Result<Vec<u8>, PluginError>; // encoded stage-specific output,
                                      // including allowed state updates/notices
}
```

### 12.6 Hook dispatch (host side)

The host keeps heterogeneous registered handlers as typed trait objects, not a
generic `dispatch<C>` trait. The scheduler matches the registered capability and
stage to its typed request, calls the corresponding method, validates the
stage-specific output and commits a stage transaction. Request/stage mismatches
must not be representable in the typed native call path. External adapters
validate the same pair at the wire boundary.

Adding a capability adds its own trait, registration entry, stage types and
namespace. Existing capability traits and hook names do not change.


## 13. Implementation plan

### 13.1 New crates

| Crate | Purpose |
| --- | --- |
| `omni_source_configurations` | Shared generic source/profile definitions extracted from `omni_configurations`, with compatibility re-exports there |
| `omni_plugin_configurations` | Activation/source declarations, manifests, capability/version/hook declarations, options and limit configuration; data only |
| `omni_plugin` | Runtime-neutral contract: typed handlers/registration, requests/results, state views/updates, host-service interfaces and errors |
| `omni_plugin_host` | Scheduling, batching, negotiated capabilities, transactional state/notices, budgets, cancellation and inference-cache orchestration |
| `omni_plugin_builtins` | Initial native JavaScript and Rust handlers, compiled into omni |
| `omni_plugin_runtime_wasm` | Later: Wasm adapter with fuel/epoch/memory limits |
| `omni_plugin_runtime_js` | Later: JavaScript adapter with interrupt/heap limits |

Extract shared source types, scaffold the configuration/contract/host foundations, then the native
pilot. Create runtime crates when implementing their adapters, not as empty
placeholders. No runtime engine dependency belongs in the contract.

Dependency direction (`A -> B` means A depends on B):

```text
omni_source_configurations -> existing low-level config/path/URL types
omni_plugin_configurations -> omni_source_configurations
omni_configurations -> omni_source_configurations + omni_plugin_configurations
omni_plugin -> omni_plugin_configurations + omni_configurations
omni_plugin_host -> omni_plugin + existing source/cache services
omni_plugin_builtins -> omni_plugin + ecosystem parsers
omni_plugin_runtime_{wasm,js} -> omni_plugin + respective engine
omni_context -> omni_plugin_host + omni_configurations
CLI/MCP composition roots -> context + builtins + enabled runtime adapters
```

`omni_configurations` owns recursive project/task/cache/env layer types and the
restricted plugin-contribution types that reuse them without allowing structural
fields such as `extends`. `omni_plugin` uses those types in hook results;
`omni_configurations` never depends on `omni_plugin` or the host. Manifest and
activation types have exactly one owner: `omni_plugin_configurations`. Keep
that crate independent of the contract and project configurations, preventing a
dependency cycle. Use existing low-level types for paths/globs where needed.
Extract generic `SourceConfig`, its variants and `SourceConfigProfile` into
`omni_source_configurations` first; preserve existing import paths through
re-exports. Subsystem-specific profiles and validators stay with their owners.
Plugin configuration owns its external-source profile and builtin wrapper;
adding `builtin` for plugins does not add it to every subsystem's source enum.

The host accepts a registry supplied by the CLI/MCP composition root; it does
not import concrete builtins or engine adapters. No-plugin invocations need not
initialize them. Source contributors bridge existing local/git materialization
to descriptors; capability negotiation precedes hook scheduling.

### 13.2 Existing crates to change

| Crate | Change |
| --- | --- |
| `omni_configurations` | Reference plugin configuration types for `plugins`; own inference controls, recursive project/task/cache/env layers and restricted inference contributions |
| `omni_context` | Marker indexing, virtual root handling, hook scheduling integration, composition, conflict reporting, provenance |
| `omni_configuration_discovery` | Optionally expose marker sets; keep one walk |
| `omni_core` | Reuse the existing generic `ExtensionGraphNode::Id`; no trait generalization is required just to introduce logical identities |
| `omni_task_executor` / `omni_cache` | Include inferred config fingerprint in effective cache identity |
| `omni_cli_core`, `omni_mcp_core` | Inspection commands/tools and diagnostics/notices wiring |
| `omni_messages` | Reuse notices/diagnostics; no new channel |
| `omni_types` | Add logical project key / provenance types if identity changes |
| `omni_remote_source_contributors` | Integrate enabled external descriptors with existing source materialization, identity and offline behavior |
| `omni_api` | Register the plugin install contributor, subsystem selection/reporting and bootstrap source services before project loading |

### 13.3 Phases

1. **Contract and composition first (no runtime).**
   - Extract shared source types with re-exports and wire/schema parity tests.
   - Land `omni_plugin_configurations`, then `omni_plugin` types and typed
     capability registration, with required/optional negotiation tests.
   - Add `plugins` config (disabled by default) and validation.
   - Add `inference` controls to `ProjectConfiguration`.
   - Introduce recursive presence-aware layers, including tasks/cache/env,
     preserving omission, explicit values and existing merge operators.
   - Replace eager explicit-file deserialization with layer loading; preserve
     extension closure, per-layer path origins, aliases and existing ordering.
     Apply defaults and final validation only after inference composition.
   - Implement conflict detection and provenance tracking.
   - Add virtual project identity (synthetic but stable logical key) and update
     extension graph + loader.
   - Tests: zero-plugin parity, extend/replace, absence vs default.

2. **Native pilots.**
   - Implement `omni_plugin_host` with frozen stage snapshots, workspace/root
     partitions, hook-set validation, atomic commits, caching and budgets.
   - Test shuffled batch scheduling, cancellation/retries, no lost state, no
     duplicate notices and invalidation of cross-root resolution snapshots.
   - Implement `@omni/javascript` (package.json; npm/Yarn/pnpm/Bun) and
     `@omni/rust` (Cargo) behind the `project_discovery` capability.
   - Add plugin source contribution/descriptor resolution without executing
     hooks during source installation; see §15 for integration tests.
   - Wire into `Context::load_discovered_project_configs`.
   - Benchmarks and regression tests.

3. **Wasm runtime.**
   - Choose engine, implement adapter, sandbox, limits, cache invalidation.
   - Conformance fixtures.

4. **JavaScript runtime.**
   - Choose engine, implement adapter, same conformance suite.

5. **Stabilize.**
    - Publish benchmarks; freeze the stable contract by omni 1.0.
    - Add version negotiation, compatibility fixtures and migration planning.
   - Add CLI/MCP inspection.
   - Keep activation explicit.

### 13.4 Integration points in current plumbing

- `omni_context::ProjectDiscovery` remains the single ignore-aware walker. Add
  plugin marker names to the same walk; do not add per-plugin walkers.
- `Context::discover_project_paths` builds candidate roots and explicit files.
  Plugin scheduling happens after explicit inference controls are read.
- `Context::load_discovered_project_configs` becomes:
  1. Load explicit files and their extension closure into recursive layers,
     preserving field presence and path origins rather than applying defaults.
  2. For roots without `replace`, invoke registered inference hooks using frozen
     stage snapshots and transactional output commits.
  3. Freeze inference fragments, then invoke dependency hooks for root-based
     edges. No later plugin stage may edit project fields.
  4. Compose inferred layers beneath explicit extension/concrete layers; apply
     defaults, resolve task extensions and final root-to-name edges, validate.
  5. Run optional read-only observation hooks and return the effective projects.
- Refactor `ProjectConfigLoader` to expose layer loading and closure/path
  resolution. Retaining its current eager `ProjectConfiguration::load` path for
  plugin-enabled composition would lose presence and reject inferred names too
  early. A no-plugin compatibility entry point may finalize the same layers.
- Cache identity: combine explicit file fingerprints, plugin artifact/module
  fingerprints, options, and normalized inferred config.

### 13.5 Risky changes requiring care

- Virtual project identity touches project graph nodes, cache keys and
  inspection. Distinguish logical ids from optional physical config files;
  `dir` remains the real project root, never a synthetic directory. Never pass
  a logical config id into file-read APIs. Reuse the core graph's associated
  identity type rather than assuming the generic trait itself must change.
- Presence-aware deserialization is a schema/loader change; tests must cover
  absence vs default values.
- Plugin output must be validated before entering graph state; malformed output
  is fatal.
- Do not reuse stale inferred output after a failed or changed run.

### 13.6 Definition of done for v1

- No enabled plugins: byte-for-byte equivalent discovery behavior and tests.
- JavaScript and Rust builtins discover projects without per-project omni files,
  with correct names, tasks and local dependency edges. JavaScript fixtures cover
  npm, Yarn, pnpm and Bun, including workspace and package-manager ambiguity;
  Rust fixtures cover Cargo packages and workspaces.
- Extend/replace and allowlists work; conflicts are deterministic.
- Wasm and JavaScript conformance fixtures match native output.
- Benchmarks meet proposed budgets or revised documented budgets.
- CLI and MCP expose effective provenance and cache status.

## 14. Versioning, compatibility and migrations

### 14.1 Stability boundary and version domains

omni is pre-v1: experimental protocol, capability and SDK releases may break
between 0.x minors. Document those breaks and recovery steps; compatibility
adapters are not mandatory during experimentation. An experimental contract must
not masquerade as stable `1.0`. Freeze protocol and the initial capability at
1.0 **no later than omni 1.0**. Stable contracts released earlier immediately
receive the same guarantees; omni being pre-v1 does not undo their stability.

Keep these version domains distinct:

| Domain | Meaning / compatibility rule |
| --- | --- |
| omni release | Host product version; gates post-v1 breaking contract introductions/removals |
| Plugin package version | Author release metadata, not a protocol promise or artifact identity |
| Envelope protocol | Semver request/response framing, negotiation, host calls and error semantics |
| Capability API | Independent semver for hooks, stages, payloads and observable behavior |
| Rust authoring SDK | Semver source API for traits, registration, types and export helpers |
| Configuration schema | Explicit schema version for persistent workspace/plugin documents when required for unambiguous migration |
| Session/cache representation | Internal versioned, rebuildable data; invalidate rather than requiring migration |

After stabilization, backward-compatible additions use minor versions, fixes use
patch versions, and breaking changes require a major version. Compatibility is
not merely successful deserialization: hook order, batch invariance, state
ownership, merge rules, permissions, defaults and existing inferred behavior
are part of the capability's contract. New hooks/features are opt-in. A new
minor must not silently change an existing plugin's interpretation or enable
new authority. A security fix may reject previously unsafe behavior; document
the exception and remediation rather than silently granting broader access.

### 14.2 Negotiation

Resolve protocol/capability requirements from the static manifest before hook
execution. The loader must be able to read the small negotiation envelope
without first interpreting a newer capability's payload format.

For each domain, select the highest mutually supported concrete version that
satisfies the plugin's explicit range and required features. A host advertises
only versions it implements natively or through tested compatibility adapters;
it cannot claim support solely because the version number falls in a range.
Capabilities must also be compatible with the selected envelope protocol.
Return the negotiated versions/features in the invocation context and expose
them in CLI/MCP inspection. Bind them to the session and include them in cache
identity. Do not renegotiate halfway through a discovery session.

A required capability without a supported intersection fails with the plugin
identity, requested range/features, host support and an upgrade/migration hint.
Optional capabilities may be skipped as in §5. No silent downgrade that drops
required behavior. Native registry validation and external module exports must
agree with manifest claims. Within a negotiated version, retain strict checks
for unknown hooks and invalid payload fields; newer features are negotiated,
not smuggled through permissive parsing.

### 14.3 External protocol versus Rust SDK

Stable Wasm/JavaScript interoperability is a wire/host-call contract, independent
of omni's private Rust structures and runtime engine choice. A supported old
module must remain usable without recompiling it merely because omni upgraded
an internal crate or engine.

The Rust authoring SDK separately follows semver once stable: compatible SDK
updates preserve supported source use; a breaking source API needs an SDK major.
This is **not** a Rust binary ABI promise or a promise that old source compiles
unchanged against every future SDK major. Native builtins compile and migrate
with omni. An older SDK may keep targeting an older supported protocol while a
new SDK exposes a redesigned source API. Publish SDK/protocol/host compatibility
tables and codemods where mechanical source migration is possible.

### 14.4 Support and deprecation window

After omni v1, introduce a breaking stable protocol/capability major only in an
omni major release. Keep its preceding contract major supported throughout that
transition host major. Remove it only in a later omni major, after deprecation
and publication of a migration path. Patch/minor host upgrades must not turn
deprecation into a load failure.

For example: omni 2.x introduces discovery API 2 while retaining API 1 through
the complete 2.x line. omni 3.0 may remove API 1 after that transition; it must
not remove it in 2.1. A contract whose replacement has not been introduced stays
supported even if omni's product major advances. Publish the matrix and planned
removal release, rather than asking users to infer it from package versions.

Compatibility adapters must preserve old observable semantics, not merely
rename fields and apply new defaults. When behavior cannot be preserved, do not
advertise support. A fundamentally incompatible redesign may require manual
migration, but still follows the documented major/deprecation policy except
for explicitly documented security necessities.

### 14.5 Migration mechanism

Provide a built-in **plan / preview / apply** migration workflow, with structured
CLI/MCP results. Exact command spelling is deferred. Planning is read-only and
reports source/target schemas, affected files, diffs, compatibility choices,
unsupported transformations and manual steps. Ordinary discovery and read-only
commands never rewrite configuration.

Explicit apply requires the files to still match the plan's fingerprints. It
validates transformed documents, makes backups and uses atomic per-file writes.
For multi-file changes, maintain a recovery journal and report partial progress
if interrupted; do not claim that separate file renames form a filesystem-wide
atomic transaction. Plans are deterministic; repeated application is idempotent.
Compose tested adjacent-version migrations where possible rather than requiring
users to install each intervening omni release.

Persistent workspace/plugin manifest schemas get explicit version markers where
interpretation would otherwise be ambiguous. The initial unversioned workspace
format is a defined legacy baseline, not a guess based on whichever fields
happen to appear. SDK source codemods are separate previewable operations; they
must not pretend to convert behavior they cannot understand.

Never silently rewrite pinned third-party artifacts, git content-store checkouts
or lock identities. A plugin requiring an artifact upgrade needs an explicit
source/pin update and normal verification. Run no arbitrary plugin-provided
migration code with host privileges; host-owned declarative transforms/codemods
and documented manual guides are the default mechanism.

Session state and inference caches are rebuildable. Invalidate and recompute
when their schema, artifact or negotiated contract changes; do not burden plugin
authors with migrating ephemeral discovery state. If durable plugin-owned data
is introduced later, it needs a separate persistence/migration contract.

When automatic conversion is unsafe or impossible, emit an actionable diagnostic
with the unsupported boundary, supported upgrade path, relevant guide and any
safe partial automation. Do not perform guessed conversions or leave a project
silently using different semantics.

### 14.6 Release gates

- Stable old-artifact fixtures run against every host update for all supported
  protocol/capability majors, including state, hooks, merge behavior and errors.
- Negotiation tests cover ranges, missing features, optional skips, incompatible
  capability/protocol pairs and manifests that overclaim their exports.
- SDK source-compatibility checks cover supported use within a stable SDK major.
- Migration golden tests cover previews, adjacent-version composition,
  idempotence, stale plans, backups, interrupted multi-file recovery and manual
  refusal paths. Test the unversioned legacy workspace baseline explicitly.
- Deprecation/removal releases and adapter coverage are published before a
  breaking host release; no runtime tier bypasses the compatibility policy.

## 15. Remote-source integration

### 15.1 Reuse the shared source lifecycle

Plugins use the same `RemoteSourceManager`, content store, pin lockfile and
union-reference GC as generators, tools, projections and packs. Do not introduce
a plugin downloader, parallel lockfile or package installer. A source shared by
a plugin and a pack is materialized once for the same repository/revision.

Normal project discovery may lazily materialize a missing **enabled** Git
plugin through the shared manager. Existing locked commits are reused, including
for branch/tag declarations; only explicit update advances a locked mutable ref.
An initial unlocked ref resolves/pins through the normal source mechanism.
Offline loading requires the selected pin and content to be available locally;
otherwise fail with the source/id and an actionable prefetch/install hint.

Host source materialization may use the network. This grants no network service
to discovery hooks: installation/bootstrap and plugin execution are separate
lifecycles. Disabled entries cause no source read, fetch, artifact loading or
runtime startup, apart from validating the workspace declaration itself.

### 15.2 Contributor, install selection and references

Add `PluginRemoteContributor` in
`crates/omni_remote_source_contributors/src/plugin.rs`, implementing the existing
`RemoteSourceContributor<TSys>` contract with stable subsystem id `plugin`.
Construct it from already-loaded activation configuration; it must not require
an inferred project graph to find its own source declarations.

Extend `omni_api::operations::remote_source::SubsystemSelection` with plugins and
thread selection through CLI/MCP request schemas and install reports. Empty
selection continues to mean all subsystems, now including enabled plugins.
Explicit generator/tool/projection-only selection does not fetch plugins.

For the selected plugin subsystem:

1. Validate activation ids and source declarations; retain enabled entries only.
2. Materialize enabled Git sources through the shared manager, passing explicit
   update intent through the existing invalidation/re-resolution mechanism.
3. Resolve/validate static manifests, module closure and selected artifact paths
   without executing plugin code. Enabled local sources use the same descriptor
   validation but do not produce remote GC references; builtins need no fetch.
4. Return every resolved Git reference for the manager-owned `refs/plugin.json`
   reference set. Reconcile only after successful source processing; failures
   must not replace a complete reference set with a partial one or run GC as if
   the failed install had succeeded.

Always run the contributor when the plugin subsystem is selected, including
when there are zero enabled plugins. Successful reconciliation must write the
empty set to release old references. Do not skip it merely because the list is
empty. Unselected subsystem reference sets remain intact, so a selected plugin
install cannot discard another subsystem's GC roots.

Lazy resolution must also retain references for its materialized sources. It
must not replace the complete plugin reference set with only the roots/plugins
needed for a particular command. Use the manager's reference coordination to
preserve the full enabled source set or safely merge newly materialized refs
until a complete reconciliation. Source GC must not reclaim an artifact while
a live plugin session is using it; coordinate active use and reclamation through
the source layer, not plugin-owned mutable state.

Disabling a plugin removes it from execution immediately; its old remote ref
is released on the next successful plugin-subsystem reconciliation. Shared
content remains while another plugin/subsystem or active invocation retains it.
Stale retained content is not activation or permission to execute.

Source install/update never calls hooks, runs package-manager installs, compiles
source code, runs build scripts or invokes migration code. Authors distribute
ready-to-load Wasm/JavaScript artifacts. Protocol negotiation/static descriptor
checks can run without executing them; dynamic export validation occurs in the
restricted loader before the first hook. An unusable artifact fails clearly.

### 15.3 Source types and artifact layout

Generic source/profile types currently live in
`omni_configurations/src/source.rs`. Move those definitions to
`omni_source_configurations`, re-exporting them from the old location. This
lets `omni_plugin_configurations` reuse local/git semantics without depending
back on `omni_configurations`. Keep wire shapes, registry stubs and existing
source consumers unchanged; validators remain subsystem-owned.

A plugin external-source profile adds activation metadata and a Git-only
optional `subdir`. Local paths must be one literal directory (validate that
constraint even if the shared local source representation permits lists).
Builtin locators are plugin-specific and do not flow into remote contributors.

Git `subdir` is a literal repository-relative directory, defaulting to the root;
reject absolute paths, traversal and symlink escapes. That directory contains
exactly one supported `plugin.omni.*` manifest, not a recursive search. Module
entries/imports stay inside that artifact directory. Workspace input services
remain separate and never expose the rest of the source checkout to hooks.

Two activation ids may select distinct subdirectories or options from one
checkout. Deduplicate materialization by the existing source identity, not by
activation id. Artifact identity includes the resolved commit, normalized
subdirectory and verified manifest/module closure digests; session/inference
identity additionally includes activation id, options and negotiated contracts.
Local sources use content fingerprints instead of Git pins and invalidate on
local edits. A declaration's package version alone is never sufficient.

### 15.4 Bootstrap plumbing and tests

The present `omni_api::operations::remote_source::open_source_store` helper
accepts a `Context`. Do not make plugin-enabled context/project loading call
back upward into `omni_api`, creating a dependency/bootstrap cycle. Extract a
lower-level source-store opener accepting workspace paths/config and system
services, or inject an already-open manager/descriptor resolver into context
loading. Retain an API convenience wrapper for existing callers. Resolve plugin
descriptors after workspace configuration is known but before project inference.

CLI and MCP compose the same source services, activation filter and runtime
registry. `omni_plugin_host` consumes resolved descriptors/source abstractions,
not CLI commands. Existing source contributors may depend on plugin configuration
data, never on executing handlers just to enumerate sources.

Required integration tests:

- Disabled Git/local entries are not read/fetched/loaded; enabled installation
  validates descriptors without any hook or build-script execution.
- A repository shared with a pack/tool or multiple plugin subdirectories
  materializes once while maintaining each subsystem's reference set.
- Locked mutable refs stay pinned on ordinary discovery/install; explicit update
  advances pins and invalidates descriptor/session/inference caches.
- Offline cache hits work; missing pins/content report actionable errors.
- Selected-subsystem install, empty enabled lists, failed reconciliation and
  active-session GC preserve/release the correct references.
- Literal subdirectory containment, duplicate manifests, missing artifacts and
  module escape attempts fail before hooks run.
- Shared source extraction preserves existing serde/schema/validation behavior
  and import paths for generators, tools, projections and packs.
