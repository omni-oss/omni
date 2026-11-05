# RFC 0024: Tiered, hook-based project discovery plugins

- **Status:** Draft <!-- Draft | Accepted | Implemented | Superseded -->
- **Owner:** TBD
- **Date:** 2026-11-05
- **Base branch:** `main`
- **Affected crates/files:** `omni_configurations` (workspace activation, plugin manifests, project inference controls and presence-aware layers), `omni_context` (discovery, loading, provenance and extension resolution), `omni_configuration_discovery`, `config_utils`, `omni_remote_source_contributors`; proposed `omni_plugin` contract and native/Wasm/JavaScript adapters; CLI and MCP inspection surfaces; schemas and discovery benchmarks
- **Related:** RFC 0011 (glob configuration), RFC 0013 (remote sources), RFC 0014 (packs), RFC 0021–0023 (diagnostics, redaction, notices and tracing)

## 1. Summary

Allow explicitly enabled plugins to infer projects from ecosystem manifests,
without requiring a `project.omni.yaml` in every directory. A Cargo plugin could
read `Cargo.toml`; a JavaScript ecosystem plugin could read `package.json`.
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

## 5. Activation, identity and delivery

Proposed `workspace.omni.yaml`:

```yaml
projects:
  include: ["crates/**", "packages/**"]
  exclude: ["**/fixtures/**"]

plugins:
  - id: cargo
    enabled: true
    source: builtin
    name: "@omni/cargo"
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
```

- `plugins` defaults to `[]`; each entry's `enabled` defaults to `false`.
  Disabled entries are not fetched, loaded or executed. Structural declaration
  errors are still reported without reading their source.
- `id` is the consumer handle, unique within the workspace. External manifests
  declare canonical `name` and metadata `version`, following RFC 0014's
  consumer-id/author-name distinction.
- `builtin` resolves a compiled registry entry. External `local`/`git` entries
  resolve a root `plugin.omni.*` manifest using existing source materialization
  and immutable lock identity (RFC 0013), not a separate downloader.
- Loading an enabled plugin with an unknown API version or unsupported runtime
  is an error. There is no silent fallback to a different tier.
- A built-in's implementation identity includes the omni build. An external
  plugin's identity includes the verified artifact and bundled module digests;
  a version string alone is never a cache or trust identity.

An external manifest might be:

```yaml
name: "@example/company"
version: 0.1.0
api_version: 1
runtime: wasm # alternatively javascript
entry: ./dist/plugin.wasm
capabilities: [project-inference]
discovery:
  markers: [company.project.json]
  inputs: [company.project.json, company.workspace.json]
```

Marker and input declarations are static data readable without starting the
runtime. JavaScript entries are self-contained bundles (or an explicitly listed,
hashed local module graph); no npm resolution, network imports, install scripts
or ambient Node installation. Source and entry paths cannot escape the
materialized source. Git source availability and offline behavior follow the
existing source mechanism. No registry is required for v1.

## 6. Discovery and the host contract

### 6.1 Hook namespaces and lifecycle

Plugins are **stateless but not sessionless**: they do not retain their own
memory between calls, and they never own mutable state. Instead, omni owns a
per-plugin, per-discovery **session state** that plugins populate and receive
back in later hooks. This lets plugins be interrupted, retried, cached and
scheduled freely while still accumulating context across a discovery pass.

The contract is a set of **stage hooks**. Every hook is **namespaced by
capability**, so adding a new capability later cannot collide with discovery
hooks or with hooks from another capability. A hook name has the shape
`<capability>.<stage>` (for example `project_discovery.infer_roots`). Plugins
declare the capabilities they implement; the host invokes only the hooks for
declared capabilities, and ignores unknown namespaces rather than failing.

The initial capability is `project_discovery`. Its stages (plugins implement
only those they need) are:

1. **`project_discovery.on_workspace`** — receive workspace options and static
   discovery scopes. May return a small, serializable session state seed. No
   filesystem access beyond declared inputs.
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
5. **`project_discovery.finalize`** — last chance to emit notices or adjust
   fragments; no new roots may be added.

Future capabilities (for example `generator_contribution` or
`tool_contribution`) would define their own namespace and stages; the host
schedules each capability's stages independently and never assumes a flat
global hook list. Session state is also partitioned by capability namespace,
so one capability's state cannot be read by another.

The host may:

- batch many roots into one `infer_roots` call instead of one call per
  directory;
- run independent plugins concurrently;
- defer or skip `resolve_dependencies` when no plugin requests it;
- cache and reuse session state fragments keyed by inputs;
- interrupt and retry a hook without leaking plugin memory, because the plugin
  holds no state of its own.

Plugins cannot assume invocation order beyond the stage order, and cannot assume
they are the only plugin. They must tolerate re-invocation and must not depend
on wall-clock or randomness.

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
5. Invoke hooks in stage order, obtaining cached or freshly inferred fragments
   and dependency references. Merge contributions deterministically, then apply
   explicit layers (§7).
6. Resolve project/task extensions in their established phases, finalize names,
   resolve inferred local dependency references to final project identities,
   and run ordinary configuration/graph validation. Produce one project model
   for all downstream consumers.

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
updated session state. Represent local dependencies by stable workspace-relative
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

Session state is owned by omni: it is serialized, size-bounded, versioned, and
included in cache identity. Plugins receive it and return a replacement; they
never mutate a shared handle. State must not contain file contents beyond what
is already tracked as declared input fingerprints, and must not be used to
bypass input tracking. State is keyed by capability namespace, so a capability
added later cannot read or overwrite another capability's state.

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
layer/patch representation, compose it, then apply defaults and validate the
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
- Session state is cached as part of the inference result; a hook that only
  needs cached state must not re-execute.

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
unsupported protocol, invalid output and inference errors must not quietly
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
counts go to tracing, not routine terminal noise.

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
2. **Native pilot:** opt-in Cargo adapter behind the same protocol and hook
   stages, host-owned candidate scan and input tracking. Specify ecosystem
   inference rules in separate adapter documentation; do not promise every Cargo
   feature here.
3. **Wasm:** versioned SDK/fixtures, pinned source loading, sandbox, resource
   enforcement and cache invalidation. No external runtime ships without its
   cancellation and containment tests.
4. **JavaScript:** the same conformance suite and SDK data model, implemented
   with a runtime selected by startup/memory/interruptibility benchmarks.
5. **Stabilize:** publish baseline measurements and protocol compatibility
   policy; retain explicit activation for every tier after stabilization.

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

Before acceptance, settle the initial built-in set, benchmark-calibrated limits,
runtime engines and whether default extend is preferable to explicit opt-in
extension. None of those decisions should weaken activation, explicit authority,
deterministic output or the ban on unbounded external execution.

## 12. Rust trait sketches (proposal)

These are illustrative, not final APIs. Native plugins implement capability
traits; external runtimes adapt the same logical calls over the wire. Hooks are
namespaced by capability, so capability traits are separate and independently
versioned. Plugins are stateless; all mutable progress lives in host-owned
`PluginState`.

### 12.1 Identity and capability registration

```rust
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PluginId(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CapabilityId(pub String); // e.g. "project_discovery"

pub struct PluginManifest {
    pub name: String,          // canonical author name, e.g. "@omni/cargo"
    pub version: String,       // metadata only
    pub api_version: u32,
    pub runtime: RuntimeKind,  // Native | Wasm | JavaScript
    pub capabilities: Vec<CapabilityId>,
}

pub trait Plugin: Send + Sync {
    fn id(&self) -> &PluginId;
    fn manifest(&self) -> &PluginManifest;
    fn capabilities(&self) -> &[CapabilityId];
}
```

### 12.2 Capability traits (plugin side)

Each capability is its own trait. Adding a capability means adding a trait and a
namespace; existing capabilities are untouched.

```rust
pub trait CapabilityPlugin: Plugin {
    fn capability_id(&self) -> CapabilityId;
}

pub trait ProjectDiscoveryCapability: CapabilityPlugin {
    fn on_workspace(
        &self,
        ctx: &HostContext,
        req: OnWorkspaceRequest,
        state: PluginState,
    ) -> Result<OnWorkspaceResponse, PluginError>;

    fn filter_candidates(
        &self,
        ctx: &HostContext,
        req: FilterCandidatesRequest,
        state: PluginState,
    ) -> Result<FilterCandidatesResponse, PluginError>;

    fn infer_roots(
        &self,
        ctx: &HostContext,
        req: InferRootsRequest,
        state: PluginState,
    ) -> Result<InferRootsResponse, PluginError>;

    fn resolve_dependencies(
        &self,
        ctx: &HostContext,
        req: ResolveDependenciesRequest,
        state: PluginState,
    ) -> Result<ResolveDependenciesResponse, PluginError>;

    fn finalize(
        &self,
        ctx: &HostContext,
        req: FinalizeRequest,
        state: PluginState,
    ) -> Result<FinalizeResponse, PluginError>;
}
```

### 12.3 Host-owned state

```rust
pub struct PluginState {
    pub capability: CapabilityId, // namespace partition
    pub version: u32,
    pub bytes: Vec<u8>,           // serialized, size-bounded, host-owned
}

pub trait StateStore: Send + Sync {
    fn load(&self, plugin: &PluginId, cap: &CapabilityId) -> Option<PluginState>;
    fn store(&self, plugin: &PluginId, cap: &CapabilityId, state: PluginState);
    fn invalidate(&self, plugin: &PluginId, cap: &CapabilityId);
}
```

Plugins never hold a `StateStore`; the host injects state into each hook call and
persists the returned replacement.

### 12.4 Host services (plugin side view)

```rust
pub trait HostFs: Send + Sync {
    fn read(&self, path: &Path) -> Result<Vec<u8>, HostError>;
    fn exists(&self, path: &Path) -> Result<bool, HostError>;
    fn canonicalize(&self, path: &Path) -> Result<PathBuf, HostError>;
}

pub struct HostContext<'a> {
    pub fs: &'a dyn HostFs,
    pub options: &'a PluginOptions,
    pub limits: &'a PluginLimits,
    pub reporter: &'a dyn NoticeReporter,
}

pub trait NoticeReporter: Send + Sync {
    fn notice(&self, notice: PluginNotice);
}
```

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
        hook: &str, // "<capability>.<stage>"
        request_bytes: &[u8],
        state_bytes: &[u8],
        limits: &PluginLimits,
    ) -> Result<(Vec<u8>, Vec<u8>), PluginError>; // (response, new state)
}
```

### 12.6 Hook dispatch (host side)

```rust
pub trait HookDispatcher: Send + Sync {
    fn dispatch<C: ProjectDiscoveryCapability>(
        &self,
        plugin: &C,
        stage: ProjectDiscoveryStage,
        ctx: &HostContext,
        req: ProjectDiscoveryRequest,
        state: PluginState,
    ) -> Result<ProjectDiscoveryResponse, PluginError>;
}
```

Adding a capability means adding a new `CapabilityId`, trait, request/response
types and hook namespace. No existing capability trait or hook name changes.
