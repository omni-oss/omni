# RFC 0010: Meta Projection Sources (bundling projections into one source)

- **Status:** Implemented <!-- Draft | Accepted | Implemented | Superseded -->
- **Owner:** TBD
- **Date:** 2026-09-03
- **Affected crates/files:** a new `omni_meta` crate (generic over the member config type; depends only on `omni_configurations`) holding the subsystem-agnostic meta-expansion core behind a `MetaExpand` trait (the DFS traversal, `qualified_id`/`::` composition, `SourceIdentity` cycle detection, `DEFAULT_META_PROJECTION_DEPTH`, the shared `--source`/`unlink` selection predicate); `omni_projection_configurations` (`projection.rs`, the leaf-XOR-meta enum + deserialize adapter; `json_schema.rs`, the `oneOf`; `id_validator.rs` is unchanged and keeps rejecting `:`); `omni_api` (`operations/projection.rs`, the projection `MetaExpand` impl, `EffectiveSource.qualified_id`, expanded-set GC, per-child `--source` matching + traversal pruning, `ProjectionSyncRequest.max_depth`); `omni_projections` (`ledger.rs`, the `::`-anchored `unlink`/`--source` predicate; `routing.rs`, `PlanInput` carries the authored `id` for the path and `qualified_id` for the ledger); `omni_configurations` (`projection.rs`, `validators.rs`); `omni-oss/json-schemas` (projection manifest schema); `omni_cli_core` / MCP surface (the `status` tree renderer, sync/status text, optional `--max-depth` flag, `--source <bundle>::<child>`)
- **Prior art referenced:** npm/pnpm meta-packages & `pnpm-workspace.yaml` catalogs, Bazel `bazel_dep`/module extensions, Nx plugin presets, Renovate `extends` presets, ESLint/Prettier shareable configs, Terraform module composition

## 1. Summary

This RFC proposes **meta projection sources**: a way for a single projection
source (git or local) to _bundle_ several other projection sources behind one
entry in `workspace.omni.yaml`. From the consumer's point of view nothing
changes. They add one entry to `projections` with an `id`, exactly as today
(RFC 0001, §5.1). The difference is entirely on the _source_ side: instead of
shipping a leaf `projection.omni.yaml` that declares `routes` for its own files,
a source may ship a **meta** manifest that declares a nested list of
`sources`, each of which is an ordinary projection source (git or local, with
its own `id`, optional inline `routes`, and possibly its own owned manifest).

At sync time omni **expands** the meta source into its constituent sources,
namespacing each child id under the meta source id with a reserved `::`
delimiter (`<meta-id>::<child-id>`), pulls/pins each child through the existing
`RemoteSourceManager`, resolves each
child's effective routes exactly as it does today, and materializes them. A
child may itself be a meta source (a bundle of bundles); expansion is a
depth-first traversal guarded by cycle detection (primary) and a generous depth
cap (backstop), mirroring omni's `run-generator` nesting model (§5.5). The
whole-run preflight, ledger, teardown, and GC operate on the _expanded_ set.

The core observation, mirroring RFC 0001, is that omni already has almost
everything needed. `WorkspaceConfiguration.projections` is
`Vec<SourceConfig<ProjectionExtra>>`; the meta manifest's `sources` field is
_the same type_. `resolve_effective_routes`, `reject_privilege_escalation`,
`plan_source`/`apply_source`, the collision preflight, the ledger, and
`retain_git_sources` already do the per-source work. The new work is: (a) a
`WorkspaceConfiguration.projections`. The new work is: (a) a
second manifest shape (`routes` XOR `sources`), (b) a deterministic expansion
pass that assigns each expanded source a system-composed `qualified_id`
(`<meta>::<child>`) for identity while leaving the authored `id` untouched for
the on-disk path, and threads git pins, and (c) making every set that is
currently keyed on _workspace_ source ids (GC `keep_ids`, git retain refs)
operate on the _expanded_ `qualified_id`s instead. The last item is the sharp
regression edge and is called out loudly (§6.5). The traversal, id composition,
cycle/depth guards, and selection predicate live behind a subsystem-agnostic
`MetaExpand` trait so the projection subsystem is the first implementation, not
the only conceivable one (§5.7, decision #13).

## 2. Motivation / Problem

RFC 0001 gives each consumer a flat list of projection sources. An organization
that wants to distribute a _standard set_ of projections, say an AI-skills
repo plus a shared-scripts repo plus a design-tokens repo, must today tell every
consumer to paste three source entries into every `workspace.omni.yaml`, keep
the three `id`s and three `rev`s in sync by hand, and re-coordinate whenever the
set grows or a repo splits. That is exactly the copy-paste-and-drift problem
RFC 0001 set out to kill, reappearing one level up: the projections no longer
drift, but the _list of projections_ does.

The desired outcome is small and symmetric with RFC 0001: an organization
publishes one repository, call it `org-standard-projections`, that declares
the bundle once, and every consumer adds a single source:

```yaml
# workspace.omni.yaml
projections:
    - source: git
      id: org-standard
      uri: https://github.com/org/standard-projections.git
      rev: main
```

The bundle repo owns the membership, the child pins, and the per-child routes.
Consumers get the whole set, updated by bumping one `rev`.

## 3. Goals / Non-goals

### Goals

- Let a projection source ship a **meta** `projection.omni.{yaml,yml,json,toml}`
  manifest that references a nested list of projection sources instead of (or
  today: exclusively distinct from) declaring its own `routes`.
- Reuse `SourceConfig<ProjectionExtra>` verbatim for the nested `sources` list,
  so a child may be `git` or `local`, carry its own `id`, optional inline
  `routes`, and/or ship its own owned manifest.
- Expand deterministically at sync time, namespacing child ids under the meta
  id with a reserved `::` delimiter (`<meta-id>::<child-id>`), and run the
  existing plan/preflight/apply/ledger pipeline over the expanded set with **no**
  change to the ledger format.
- Keep each source's authored `id` untouched, so RFC 0001 #23 ("namespace = id")
  holds and the `namespaced` on-disk path is unchanged. Carry the composed
  `<meta>::<child>` form in a separate, system-assigned `qualified_id` used only
  for identity: the ledger key, GC, uniqueness, and teardown. `::` is a delimiter
  `validate_projection_id` forbids in authored ids, so a `qualified_id` is
  unambiguously machine-composed and can never collide with a hand-authored id.
- Let `sync --source` and `unlink` accept either a whole bundle or a single
  child subtree through one shared `::`-anchored predicate
  (`qualified_id == arg || qualified_id.starts_with("arg::")`), fetching only the
  selected subtree.
- Keep the untrusted half untrusted: routes declared _inside_ a meta manifest
  (whether inline or via a child's owned manifest) are subject to the same
  `reject_privilege_escalation` floor as any owned manifest. A bundle can never
  grant itself or a child `allow_omni_config` / `allow_git`.
- Preserve idempotency, teardown (`unlink`/`prune`), git GC, and `--dry-run`
  correctness on the expanded set.
- Keep the consumer-facing wire shape (`workspace.omni.yaml`) byte-for-byte
  unchanged; keep existing leaf `projection.omni.yaml` manifests parsing
  unchanged.

### Non-goals

- **No new workspace-config surface.** The meta capability lives entirely in the
  source-owned manifest. `WorkspaceConfiguration.projections` is untouched. This
  is the user's explicit framing: "just another source with its own projection
  config."
- **Bounded recursion, not unbounded.** A meta manifest may reference sources
  that are themselves meta (a bundle of bundles). Expansion is guarded by cycle
  detection (primary) and a generous, overridable depth cap (backstop), mirroring
  the `run-generator` model (§5.5, decision #6). A cyclic or pathologically deep
  bundle graph is a hard error, never a hang or an unbounded fetch fan-out.
- **No workspace-level `routes` override of a meta source.** A workspace source
  whose manifest resolves to _meta_ may not also carry `routes` (there is no 1:1
  route set to override). This is a hard error, not silent (decision #4).
- **No content transformation.** Same as RFC 0001. Projections link or copy
  verbatim; meta only changes _which_ sources contribute.

## 4. Background: current mechanisms

Everything below is present today and is what this RFC composes on top of.

### 4.1 A projection source is `SourceConfig<ProjectionExtra>`

`WorkspaceConfiguration.projections: Vec<ProjectionSourceConfiguration>`
(`omni_configurations/src/workspace_configuration.rs` L52–54), where
`ProjectionSourceConfiguration = SourceConfig<ProjectionExtra>`
(`omni_configurations/src/projection.rs` L5). `ProjectionExtra`
(`omni_projection_configurations/src/projection.rs` L280–286) is:

```rust
pub struct ProjectionExtra {
    #[serde(deserialize_with = "validate_projection_id")]
    pub id: String,
    #[serde(default)]
    pub routes: Option<Vec<Projection>>,
}
```

`SourceConfig<E>` (`omni_configurations/src/source.rs` L25–66) is the unified
`local`/`git` origin generic introduced by RFC 0001. This is the exact type a
meta manifest's `sources` field will reuse.

### 4.2 Owned manifests are discovered and are leaf-only today

`discover_owned_manifest` (`omni_api/src/operations/projection.rs` L597–627)
globs the source root for `projection.omni.{ext}` (honoring `.omniignore`) and
parses it into:

```rust
pub struct OwnedProjectionConfiguration {
    pub routes: Vec<Projection>,
}
```

(`omni_projection_configurations/src/projection.rs` L292–294). Its `JsonSchema`
is hand-written in `json_schema.rs` because the `Projection` enum is internally
tagged with per-variant `deny_unknown_fields` (schemars can't derive it).

### 4.3 Route resolution and the privilege floor

`resolve_effective_routes` (`operations/projection.rs` L564–593) decides a
source's routes:

- workspace `routes` present → override the owned manifest wholesale (a debug
  log notes the override);
- workspace `routes` absent → use the owned manifest's `routes`, first running
  `reject_privilege_escalation`;
- neither → error.

`reject_privilege_escalation` (L634–644) rejects any owned route that sets
`allow_omni_config`/`allow_git`; those flags are honored **only** from workspace
config. This is the trust boundary a meta manifest must inherit.

### 4.4 Fetch, pin, plan, apply, GC

`handle_projection_sync` (L150–381) runs three phases:

1. For each workspace source: resolve `source_root`+`git_pin` (git via
   `RemoteSourceManager::pull_git_repo` / `locked_commit`, local via
   `single_path`), resolve routes, `plan_source`.
2. Whole-run preflight: `collision_conflicts` + `existing_file_conflicts` over
   _all_ planned items across _all_ sources; any conflict aborts before writes.
3. Apply each source's plan, rewrite that source's ledger links
   (`ledger.links_mut().retain(|l| l.source_id != *prep.id)` then extend).

Two GC sets key on **workspace** source ids/refs and only run on a full
(`req.source.is_none()`) sync:

- `keep_ids: HashSet<&str> = sources.iter().map(source_id).collect()` →
  `retain_sources` reconciles away ledger links whose `source_id` is not in
  `keep_ids` (L354–361).
- `all_git` (L170–176) → `retain_git_sources` GCs git checkouts not in the set
  (L373–377).

These two sets are the load-bearing correctness surface for meta expansion
(§6.5).

### 4.5 Ids already support nested namespaces

`validate_projection_id` (`id_validator.rs`) accepts `/`-delimited multi-segment
ids (`@scope/name/sub`, `a/b/c`), rejects `..`/absolute/empty segments and
Windows reserved names (and rejects `:` outright, the `rejects_illegal_characters`
test covers `a:b`), and stores the canonical `/`-form. Because `:` is not a legal
author-written id character, `::` is available as a system-reserved delimiter for
machine-composed meta ids that can never collide with a hand-authored id
(decision #3).

## 5. Proposed design

### 5.1 Manifest shape: leaf XOR meta

The manifest is genuinely a sum type. A source ships _either_ routes for its own
files _or_ a list of sub-sources, never both, so the domain type should be an
**enum**, making the illegal states (both, neither) unrepresentable rather than
relying on a runtime check over a struct of two `Option`s:

```rust
pub enum OwnedProjectionConfiguration {
    Leaf { routes: Vec<Projection> },
    Meta { sources: Vec<SourceConfig<ProjectionExtra>> },
}
```

The only real question is the _wire_ tagging, and it has one hard constraint:
the leaf form `{ routes: [...] }` already ships today (RFC 0001), authored by
third-party source repos, so the wire shape must stay tag-free and
backward-compatible. That rules out the two obvious derives:

- `#[serde(untagged)]`, back-compatible but produces opaque "data did not match
  any variant" errors and interacts badly with `deny_unknown_fields` (the same
  reasons `Projection`'s schema is already hand-written).
- `#[serde(tag = "kind")]` (internally tagged, as `Projection` uses `strategy`)
  is clean and derives `Deserialize` for free, but **breaks the shipped
  `{ routes }` wire shape** by requiring a new `kind: leaf` discriminator on
  every existing manifest.

The resolution keeps the clean enum as the public type and controls only its
_deserialization_: deserialize into a private, `deny_unknown_fields` wire adapter
with two optional fields, then convert into the enum with the exactly-one check.

```rust
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    #[serde(default)] routes:  Option<Vec<Projection>>,
    #[serde(default)] sources: Option<Vec<SourceConfig<ProjectionExtra>>>,
}

impl TryFrom<RawManifest> for OwnedProjectionConfiguration { /* exactly-one */ }
```

This is the deserialize-through-intermediate pattern, and it mirrors how the
codebase already handles `Projection` (internally-tagged `Deserialize` derive
with per-variant `deny_unknown_fields`, plus a hand-written `JsonSchema`): the
domain type stays a proper enum, the wire adapter absorbs the format quirk, and
the `JsonSchema` in `json_schema.rs` gains one `oneOf` between the leaf and meta
shapes. The adapter gives a precise error ("`projection.omni` must declare
exactly one of `routes` or `sources`") and keeps unknown-field rejection exact.

> Note: existing leaf manifests (`{ routes: [...] }`) parse unchanged because
> `routes` stays a top-level key. This is a compatible extension, not a break.
> The tag-free adapter is the decided approach, because the manifest schema is
> published externally and omni has no config-migration path (decision #2); a
> tagged `#[serde(tag = "kind")]` enum was rejected for exactly that reason.

### 5.2 Expansion algorithm

Expansion happens in Phase 1 of `handle_projection_sync`, replacing the current
"for each workspace source" loop with a deterministic depth-first traversal that
flattens meta sources into leaves. In pseudocode:

```
fn expand(sources, prefix, parent_root, ancestors, depth, select) -> Vec<EffectiveSource>:
    out = []
    for src in sources:                       # config order preserved
        (root, pin, identity) = materialize(src, parent_root)  # git pull / local (contained)
        qid = join(prefix, src.id)             # qualified_id "<a>::<b>::..." (system-only)

        # Selection pruning (§5.6): skip subtrees that cannot contain `select`.
        if not selected_or_on_path(qid, select): continue

        guard_cycle(identity, ancestors, qid)      # PRIMARY guard (decision #6)
        guard_depth(depth, qid)                    # BACKSTOP guard (decision #6)

        manifest = discover_owned_manifest(root)
        # Only a depth-0 *workspace* `routes` override is trusted; everything
        # reachable through a manifest (inline or owned, any depth) is not.
        trusted = (depth == 0 and src.routes.is_some())

        match (src.routes, manifest):
            (Some(routes), _):                 # override wins
                if manifest is Meta: error   # can't override a meta wholesale (#4)
                if not trusted: reject_privilege_escalation(qid, routes)  # (#5)
                out.push(leaf(id=src.id, qualified_id=qid, root, pin, routes))
            (None, Meta(children)):            # recurse into the bundle
                out += expand(children, qid, root,
                              ancestors + [identity], depth + 1, select)
            (None, Leaf(routes)):
                reject_privilege_escalation(qid, routes)   # owned => untrusted (#5)
                out.push(leaf(id=src.id, qualified_id=qid, root, pin, routes))
            (None, None):
                error "no routes and no manifest"
    return out

# top level:
effective = expand(workspace_sources, prefix="", parent_root=workspace_root,
                   ancestors=[], depth=0, select=req.source)
assert_unique_qualified_ids(effective)    # over the fully EXPANDED set
```

Key points:

- **Deterministic order.** A pre-order DFS over declared lists (workspace order,
  then each meta's child order). No set iteration influences link order.
- **Authored `id` vs. `qualified_id`.** Each source keeps its authored `id`
  verbatim: it is validated by `validate_projection_id` (which rejects `:`), and
  it is what the `namespaced` strategy uses for the on-disk path, so RFC 0001 #23
  ("namespace = id") holds unchanged. The system separately composes a
  `qualified_id` = `format!("{prefix}::{id}")` (or just `id` at the root) purely
  for identity: the ledger key, GC, uniqueness, and teardown. The composed string
  is a **system artifact** and is never fed back through `validate_projection_id`.
  Only the authored segments are validated, then joined (compose-after-validate,
  decision #3). Because `::` is unauthorable, a `qualified_id` can never clash
  with a declared id, and uniqueness is checked over the `qualified_id`s of the
  _expanded_ set, not the workspace list.
- **`::` never reaches disk.** The `qualified_id` prefix is identity only.
  `namespaced_pair` (`routing.rs`) receives the authored `id`, so a child
  `skills` under bundle `org-standard` (qualified `org-standard::skills`)
  materializes at `<target>/skills`, exactly where a directly-declared `skills`
  would. Two bundles that both ship a `skills` child therefore resolve to the
  same `namespaced` path; that path collision is caught by the whole-run
  preflight (§6.5), not prevented structurally.
- **Local child roots resolve relative to the parent's root** (`parent_root`),
  not the workspace root, and are realpath-contained within it (§5.3). A git
  child is pulled through the same `RemoteSourceManager`.
- **Route resolution reuses `resolve_effective_routes`** semantics: a source's
  own `routes` (inline in a meta manifest, or a workspace override) take
  precedence over its owned manifest, with the trust caveat below.
- **Cycle + depth guards** are detailed in §5.5.

### 5.3 Trust and privilege model

The single most important invariant: **everything reachable through a meta
manifest is untrusted.** Concretely:

- A child's inline `routes` (declared in the meta manifest) are _not_ workspace
  config. They arrived over the network or from a vendored repo. Therefore
  `reject_privilege_escalation` **must** run over them, unlike top-level
  workspace `routes` which are trusted. This means `resolve_effective_routes`
  needs a "trusted vs untrusted routes" distinction, or (cleaner) the expansion
  pass runs `reject_privilege_escalation` on every child's resolved routes
  regardless of whether they came from inline or owned manifest. Recommended:
  apply the floor to _all_ expanded-child routes unconditionally (decision #5).
- Local child paths are realpath-contained within the meta source root (reusing
  the engine's existing `containment` helpers, `omni_projections/containment.rs`).
  A child `path: ../../etc` is rejected, exactly as a route source entry that
  escapes its source root is rejected today.
- `allow_omni_config`/`allow_git` remain honored only from the _workspace_
  source's own top-level `routes`. A meta bundle, no matter how deep, can never
  turn them on.

This keeps the meta feature strictly weaker in authority than a hand-written
workspace list: a consumer who bundles is trusting the bundle author for
_membership and file content_, but the bundle still cannot touch the control
plane.

### 5.4 Git fetch, pinning, GC, determinism

- Each git child is pulled and pinned through the same `RemoteSourceManager`
  instance already built in `build_remote_manager`. Child pins are recorded
  per-ledger-link exactly as leaf pins are today (the ledger `source_pin`
  field); the ledger format is unchanged.
- The meta source itself, when git, is pulled and pinned so its manifest can be
  read, but it contributes **no links of its own** (it has no `routes`). Its pin
  matters only for cache-freshness of the manifest read and for GC retention.
- **GC retention must use the expanded set** (see §6.5). `keep_ids` becomes the
  set of expanded `qualified_id`s; `retain_git_sources` must be fed every git
  ref that was pulled, the meta repos and every git child. Otherwise a full sync
  would delete the children it just fetched and reconcile away their links.
- `--update` propagates to children: re-resolving a meta source's mutable rev
  re-reads its manifest, which may bump child revs, add children, or drop
  children. Order is deterministic (parents before children), and the re-read
  manifest is authoritative for what gets linked this run. A child dropped by a
  re-read is removed only on a **full** sync, where it falls out of `keep_ids`; a
  _targeted_ `sync --update --source <bundle>` does not remove it (targeted syncs
  never GC, #11) but must emit a warning naming the now-absent child and its
  lingering links and pointing the user at a full sync to reconcile (decision
  #16).

### 5.5 Bounded recursion: cycle detection + depth cap

A child may itself resolve to a meta manifest, so `expand` recurses (§5.2). Two
guards keep that recursion total, mirroring omni's existing nested-subsystem
model: `bridge_rpc_runner::DEFAULT_MAX_DEPTH` (64) is documented as _"Static
cycle detection is the primary guard; this bound is a defense-in-depth
backstop,"_ and `generator_run` pairs a `visited`-set cycle check with an
overridable `max_depth: Option<usize>` (`GeneratorRunRequest`,
`DEFAULT_MAX_GENERATOR_DEPTH`). Meta expansion adopts the same two-guard shape.

**The primary guard is cycle detection.** Each node carries the chain of _source
identities_ of its meta ancestors (the current DFS path, not a global visited
set). Before expanding a source, if its identity already appears in that chain,
expansion aborts with a cycle error naming the id chain
(`org → org::team-a → org::team-a::org`). A `SourceIdentity` is:

- **git:** the normalized `uri` plus the declared `rev` (`(uri, rev)`);
- **local:** the canonicalized absolute `source_root`.

Because the check is scoped to the ancestor _path_, a **diamond** is allowed: an
org bundle may reference `team-a` and `team-b`, both of which reference the same
leaf `shared-skills`. That yields two distinct expanded ids
(`org::team-a::shared-skills`, `org::team-b::shared-skills`) and two independent
link sets, not a cycle. Only a true back-edge (a source that is its own
ancestor) is rejected.

> Subtlety: a repo that references itself at a _different_ rev
> (`A@v1 → A@v2 → A@v3 → …`) is not caught by identity (each `(uri, rev)` is
> distinct) precisely because each rev is genuinely different content. That case
> is exactly what the depth cap backstops.

**The backstop guard is the depth cap.** `depth` increments on each meta descent. Beyond
`DEFAULT_META_PROJECTION_DEPTH` (a new constant, set to **16**) expansion aborts.
The value is deliberately _not_ an alias of the generator/bridge `64`: those
recursions are in-process and cheap, whereas each meta descent is a **git
fetch**, so a runaway graph at 64 would mean up to 64 sequential clones before
the backstop trips. 16 is still far deeper than any real bundle (2–3 levels),
so it never interferes legitimately, but it fails an adversarial or misconfigured
fan-out roughly four times sooner. As with generators, `ProjectionSyncRequest`
gains an optional `max_depth: Option<usize>` so a legitimately deep bundle graph
can raise it; `None` uses the default. The _shape_ mirrors `run-generator`
(cycle-set primary guard + overridable cap); only the constant differs.

The id-composition scheme (`<a>::<b>::<c>`) supports arbitrary depth without new
id machinery. The composed segments join with `::`, so recursion needs only the
two guards and the recursive call in `expand`.

### 5.6 CLI / API / MCP surface

No new commands. `omni projection sync|status|unlink|prune` are unchanged in
shape. The human-readable `status` (and `sync`/`--dry-run`) output renders the
expanded members as a tree grouped by `qualified_id` (§5.6.3); the
machine-readable API/MCP report stays a flat entry list of full `qualified_id`s.
Two behavior changes are driven by one shared `::`-anchored predicate: `unlink`
(§5.6.1) and per-child `--source` selection (§5.6.2).

#### 5.6.1 `unlink <meta-id>`: a clean prefix teardown

**Why a change is needed.** `handle_projection_unlink` today tears down every
ledger link whose `source_id` equals the argument. It calls
`omni_projections::unlink`, which takes a `source_id: &str` and builds the
predicate `|l| l.source_id == source_id` internally (`ledger.rs`, the `unlink`
fn at L159–177, predicate at L174). A meta bundle contributes no link under its
own id: all of its links carry composed `qualified_id`s like `org-standard::skills`
(which is what the ledger `source_id` field stores). So with exact matching,
`omni projection unlink org-standard` matches zero links and silently no-ops,
stranding the entire bundle on disk and in the ledger. The user's natural mental
model ("unlink the thing I added to my config") must tear down the whole bundle.

**Why `::` makes this trivial.** Because every `qualified_id` is composed with
the reserved `::` delimiter (§5.2, decision #3) and `validate_projection_id`
forbids `:` in any author-written id, a bundle child is _unambiguously_
identifiable: its ledger `source_id` (a `qualified_id`) begins with `<meta-id>::`.
The fix widens `unlink`'s internal predicate (or gives it a predicate parameter)
to the `::`-anchored prefix match, the _same_ predicate `sync --source` uses
(§5.6.2, decision #11), with no config read and no exclusion logic:

```rust
|l| l.source_id == source_id
    || l.source_id.starts_with(&format!("{source_id}::"))
```

There is no false-positive to guard against. A `/`-delimited id could not
distinguish a meta child `foo/bar` from an independently-declared top-level
`foo/bar` (both legal under RFC 0001 #26) and would force `unlink` to re-read
config to disambiguate; `::` removes that ambiguity structurally. A user simply
_cannot_ author an id containing `::`, so `<meta-id>::` can only ever prefix that
bundle's own machine-composed children.

**Ledger-only and format-stable.** This keeps `unlink` purely ledger-driven
(no config load, no fetch) and requires no ledger-format change (decision #10):
the reserved delimiter is already embedded in the stored `source_id`, so
parentage is recoverable from the id string alone. Recovery from a mistaken
`unlink` remains as strong as today. `sync` rebuilds the desired link set from
committed config + pins (RFC 0001 #30), and `copy`-kind links with local edits
are preserved by `unlink`'s existing modified-copy handling (the
`unlink_preserves_modified_copy` test).

#### 5.6.2 `sync --source <arg>`: whole bundle or a single child subtree

`--source` today is an exact match against a workspace source id and doubles as
the GC gate (`full_sync = req.source.is_none()`; the two reconcilers run only on
a full sync). Meta generalizes the _match_, not the gate. `--source <arg>`
selects every expanded source whose `qualified_id` satisfies the same predicate
`unlink` uses:

```rust
qualified_id == arg || qualified_id.starts_with(&format!("{arg}::"))
```

So `<arg>` names a single source or a bundle subtree, and `sync --source` and
`unlink` share one mental model. `--source org-standard` syncs the whole bundle;
`--source org-standard::skills` syncs just that child; `--source
org-standard::team-a` syncs a sub-bundle's whole subtree.

**Fetch scoping is by pruning, not fetch-then-filter.** `qualified_id`s only
exist after expansion, and expansion means git pulls, so selecting by expanding
everything first would fetch unrelated bundles and break the "targeted sync
touches nothing unrelated" guarantee. Instead the traversal prunes:

- **Top-level pre-filter.** A composed arg's first `::` segment is the top-level
  id (authored ids never contain `::`), so the Phase 1 loop enters only the
  workspace source whose `id` equals `first_segment(arg)`; every other top-level
  source is skipped with no fetch.
- **Within-bundle prune.** Reading the selected bundle's manifest is cheap (its
  repo is already pulled); from there the traversal descends only into children
  on the path to the target or inside the selected subtree
  (`selected_or_on_path` in the §5.2 pseudocode). `--source org-standard::skills`
  pulls `org-standard` and `skills` only, not `scripts`/`agents`.

**GC gate stays binary.** `full_sync = req.source.is_none()` is unchanged, so any
`--source`, simple or composed, is targeted and never GCs. Only the applied
leaves' ledger links are rewritten (`retain(|l| l.source_id != qualified_id)`
then extend); siblings and unrelated sources keep their links and checkouts.
This is why per-child selection needs no GC rework: the destructive reconcilers
remain reachable only from a full, unfiltered sync. The blast radius of a
pruning bug is correspondingly bounded. The worst it can do is over-fetch (slow)
or apply the wrong leaf set (wrong links, fixed by a re-sync); it cannot delete
unrelated sources' links or checkouts.

**Matched-nothing is an error, not a no-op.** Today an unknown `--source`
silently no-ops (the loop skips everything, `full_sync` is false, exit 0). With
selection that becomes a hard error: `first_segment(arg)` that is not a known
top-level id fails before any fetch (`no projection source 'nonsense'`); a first
segment that matches a bundle but a child path that does not resolve fails after
the bundle manifest is read (`no source matching 'org-standard::typo' in bundle
'org-standard'`).

#### 5.6.3 `status` tree rendering

The human-readable `status` output groups the expanded members into a tree
derived from the `::` structure of their stored `qualified_id`s. Because `::` is
unauthorable, the grouping is reconstructed from the ledger alone, with no config
read, so `status` stays ledger-and-filesystem only as it is today. A bundle is a
synthesized group node (it owns no link of its own); each member row shows its
local id, and a bundle-of-bundles nests one indent per `::` segment. A group
header shows a rollup of its descendants' states as counts (`(2 ok, 1 drifted)`),
which stays truthful under mixed states and doubles as a size signal. The
bundle name in a group header is rendered in bold via `owo-colors` (the styling
crate already used across `omni_cli_core`, for example in `generator list`), so a
synthesized bundle node reads as visually distinct from a leaf member, which
stays unstyled. Rows sort
by `qualified_id` path so groups cluster deterministically; a plain top-level
source with no `::` renders as a flat leaf. The tree lives in the CLI renderer,
not in `omni_meta` or the API report (decision #17):

```
org-standard              (2 ok, 1 drifted)
  agents                  ok
  scripts                 ok
  skills                  drifted
  team-a                  (1 broken)
    shared-skills         broken
design-tokens             ok
```

### 5.7 A subsystem-agnostic `MetaExpand` seam

The expansion machinery splits cleanly into a part that has nothing to do with
projections and a part that is entirely projection domain. The generic part is:
the DFS traversal, `qualified_id`/`::` composition and the authored-`id` split,
`SourceIdentity` and ancestor-path cycle detection, the depth cap, and the
`--source`/`unlink` selection predicate + pruning. The projection-specific part
is: route resolution, `reject_privilege_escalation` and the
`allow_omni_config`/`allow_git` floor, the ledger and its `source_id`, GC
`keep_ids`/`retain_git_sources`, and the `namespaced` path shape.

The generic part lives in a dedicated `omni_meta` crate behind a trait, generic
over the member config type, so the projection subsystem is the first
implementation, not a hard-wired one. `omni_meta` depends only on
`omni_configurations` (for `SourceConfig<E>`); it knows nothing about routes, the
ledger, or projections, and a crate boundary makes that a compile-time fact
rather than a convention (decision #15):

```rust
enum Node<Leaf, Extra> {
    Leaf(Leaf),
    Meta(Vec<SourceConfig<Extra>>),
}

trait MetaExpand {
    type Extra;   // ProjectionExtra today; NoExtra for a future tool/generator
    type Leaf;    // the resolved payload the caller consumes

    // Materialize (fetch/pin) and classify one source. The I/O lives here, so
    // the traversal itself is pure and unit-testable with a fake impl.
    fn classify(
        &self,
        src: &SourceConfig<Self::Extra>,
        qualified_id: &str,
        parent_root: &Path,
        depth: usize,
    ) -> Result<(Node<Self::Leaf, Self::Extra>, SourceIdentity, Pin)>;
}
```

The traversal returns `Vec<EffectiveSource<Leaf>>` (`id`, `qualified_id`, root,
pin, `Leaf`). The projection impl of `classify` runs `discover_owned_manifest` +
`resolve_effective_routes` + `reject_privilege_escalation`. Putting the fetch
I/O behind `classify` is a real testability win _today_: cycle detection, depth
capping, id composition, and selection pruning are tested with a trivial fake
`classify` and no git or filesystem (§6.3).

**The honest limit (decision #13).** Keeping this seam does _not_ give tools and
generators meta bundles for free. They use `SourceConfig<NoExtra>` and have no
owned-manifest-reconfigures-source layer: omni fetches a generator/tool source
and reads `generator.omni.*` / `tool.omni.*` as the leaf config itself, not a
second manifest that can list further sources the way `discover_owned_manifest`
does for projections. Before either subsystem can bundle, that owned-manifest
layer must be built for it. What the seam guarantees is that when it is, the
traversal, id composition, cycle/depth guards, and selection are already present
and tested.

## 6. Mandatory analysis

### 6.1 Existing mechanisms (reuse vs. extend vs. add)

| Concern                     | Mechanism today                                    | Meta approach                                                           |
| --------------------------- | -------------------------------------------------- | ----------------------------------------------------------------------- |
| Source origin (git/local)   | `SourceConfig<ProjectionExtra>`                    | **Reuse verbatim** as the meta manifest's `sources` element type        |
| Manifest discovery          | `discover_owned_manifest`                          | **Extend** the parsed type to leaf-XOR-meta; discovery loop unchanged   |
| Route resolution            | `resolve_effective_routes`                         | **Reuse**; add unconditional privilege floor on expanded-child routes   |
| Fetch/pin/GC                | `RemoteSourceManager`, `retain_git_sources`        | **Reuse**; feed the _expanded_ ref/id sets                              |
| Plan/preflight/apply/ledger | `plan_source`/`collision_conflicts`/`apply_source` | **Reuse unchanged** over the expanded leaf set                          |
| Id safety/namespacing       | `validate_projection_id` (`/`-delimited)           | **Reuse**; compose with the reserved `::` delimiter (`<meta>::<child>`) |

The only genuinely new code is (a) the manifest enum + its deserialize adapter,
(b) the subsystem-agnostic `MetaExpand` traversal (cycle + depth guards, the
`qualified_id`/`::` composition, and the shared `--source`/`unlink` selection
predicate), (c) the projection `MetaExpand` impl plus widening
`keep_ids`/git-retain/`unlink` to the expanded `qualified_id` set, and (d)
threading two ids through `PlanInput` (authored `id` for the `namespaced` path,
`qualified_id` for the ledger). `validate_projection_id` and `namespaced_pair`
need no delimiter changes: the authored `id` never contains `::`, and it alone
feeds the path. Everything downstream of a resolved leaf source is untouched.

### 6.2 Maintenance cost

- One new manifest enum (`Leaf | Meta`) with a small deserialize adapter and a
  `oneOf` in the hand-written schema. The schema file already carries this kind
  of complexity, so the marginal cost is small but real.
- A new invariant future contributors must remember: **any set keyed on
  projection source id must be the expanded set.** This is easy to get wrong
  silently; it should be encoded in one place (a single `expand()` returning the
  effective sources, with `keep_ids`/git-refs derived from _its_ output) rather
  than recomputed at each call site.
- No new dependencies. No lockfile-format or ledger-format change.

### 6.3 Testability

- **Config unit tests** (`omni_projection_configurations`): leaf still parses;
  meta parses; `routes`+`sources` together is rejected; neither is rejected;
  meta with an inline-privilege-escalating child route is rejected; schema
  `oneOf` snapshot.
- **Expansion unit tests** (`omni_api/operations/projection.rs`, alongside the
  existing `owned_manifest_is_inherited_*` tests): id composition; deterministic
  pre-order; local-child-relative-root + containment; child-owned-manifest
  inheritance; expanded-set uniqueness collision.
- **Recursion tests**: a bundle-of-bundles expands to the right namespaced leaf
  set; a **diamond** (same leaf reached via two bundles) is allowed and yields
  two distinct ids; a **cycle** (`A → B → A`, git and local identities) is a hard
  error naming the chain; the **depth cap** aborts a too-deep graph and
  `max_depth` raises it; a self-reference at differing revs is caught by the cap.
- **Trust tests**: a meta child (inline or owned, at any depth) cannot set
  `allow_omni_config`/`allow_git` (parallel to the existing
  `owned_routes_cannot_escalate_privileges` test); a local child escaping its
  parent root is rejected.
- **`unlink` / `--source` tests**: `unlink <meta-id>` tears down all
  `<meta-id>::*` children; an independently-declared top-level source (e.g. `foo`
  and a separate `foobar`) is untouched; `--source <meta>` selects the whole
  bundle, `--source <meta>::<child>` selects one child and prunes sibling
  fetches, an unknown top-level arg errors before any fetch, an unresolved child
  path errors naming the bundle, and a targeted sync never GCs.
- **Filesystem-free traversal tests**: because fetch/classify sits behind the
  `MetaExpand` trait (§5.7), cycle detection, depth capping, `qualified_id`
  composition, and selection pruning are unit-tested with a fake `classify` and
  no git or real filesystem.
- **Id / path tests**: `::` (and bare `:`) rejected in author-written ids; a
  bundle child `skills` (qualified `org-standard::skills`) materializes its
  `namespaced` link at `<target>/skills` (authored id only, `::` never on disk);
  two bundles shipping a `skills` child at the same target trip the whole-run
  preflight with an error naming both bundles and the contested path.
- **Integration/e2e** (`tests/omni-tests`): a git meta repo whose manifest lists
  two git children, synced end-to-end; full-sync GC does _not_ delete children;
  `unlink <meta-id>` tears down all children; `--dry-run` lists expanded links.
- **Hard-to-test**: real multi-repo git fetch fan-out ordering under `--update`;
  covered with `InMemorySys` fixtures plus a smaller number of real-git e2e
  cases, following RFC 0001's split.

### 6.4 Integration points

- `handle_projection_sync` Phase 1 loop → replaced by the `MetaExpand` traversal
  (§5.2–§5.5, §5.7); Phases 2–3 consume `Vec<EffectiveSource>` (today's
  `PreparedSource` fields plus `qualified_id`). `ProjectionSyncRequest` gains
  `max_depth: Option<usize>`; `--source` matching moves to the shared
  `::`-prefix predicate with traversal pruning (§5.6.2).
- `keep_ids` (L354, L286) and `all_git`/retain refs (L170–176, L373–377) →
  derived from `expand()` output (every expanded `qualified_id`; every pulled git
  ref, including meta repos and git children at every depth).
- `handle_projection_unlink` → unchanged control flow; the ledger predicate in
  `omni_projections::unlink` widens to a plain `::`-anchored prefix match
  (§5.6.1, shared with `--source` §5.6.2). No config read, no exclusions.
- `validate_projection_id` (`id_validator.rs`) → unchanged; it keeps rejecting
  `:` in author input, which is exactly what makes `::` a safe system delimiter.
  `namespaced_pair` (`routing.rs`) → path logic unchanged; `PlanInput` gains a
  split between the authored `id` (the `namespaced` path) and the `qualified_id`
  (recorded as the ledger `source_id`).
- `OwnedProjectionConfiguration` + `json_schema.rs` + the `Projection` schema
  emitted via `handle_config_schema` (`SchemaKind::Projection`) → the schema test
  `projection_schema_is_the_owned_manifest_shape` must be updated to allow the
  meta shape.

### 6.5 Potential regressions

These are the silent-failure hazards; they dominate the risk of this change.

- **GC deletes freshly-fetched children (severe, silent).** If `keep_ids` stays
  the workspace list, a full `sync` classifies every `org-standard::*` ledger
  link as an orphan and reconciles it away; if `retain_git_sources` keeps only
  the workspace git refs, the children's checkouts are GC'd. Both must switch to
  the expanded set. This is the single most dangerous edge and must have a
  dedicated full-sync-does-not-GC-children e2e test.
- **`unlink <meta-id>` no-ops (data-stranding).** Exact-id match leaves all
  `<meta-id>::*` links behind. Resolved by the `::`-anchored prefix match
  (§5.6.1, decision #7); because `::` is unauthorable, the match has no
  false-positive to guard against. Needs the `unlink` tests in §6.3.
- **Cyclic or unbounded bundle graphs (hang / fetch storm).** A bundle that
  references itself, or nests pathologically deep, could loop or fetch
  unboundedly. Mitigated by the two guards (§5.5): ancestor-path cycle detection
  (primary) and the `DEFAULT_META_PROJECTION_DEPTH` cap (backstop, catching the
  same-repo-different-rev case identity cannot). Both must have tests; the cap
  must fail _fast_, before fetching the next level.
- **Namespaced path collisions across bundles (preflight-caught).** Because a
  bundle child's `namespaced` path uses its authored `id`, not the composed
  `qualified_id` (decision #12), two bundles that each ship a `skills` child at
  the same target resolve to the same on-disk path. The `qualified_id`s stay
  unique so the uniqueness check passes, but the paths collide; the whole-run
  preflight catches it and aborts before any writes. The error must name **both
  bundles and the contested path** (decision #14), because the consumer authored
  neither bundle and cannot open the manifests to diagnose it. This is a
  deliberate trade of structural collision-proofing for a clean on-disk path (a
  bundled source lands where a direct one would); the safety net is the
  preflight, which is loud, never silent.
- **Per-child `--source` pruning (bounded).** A pruning bug could over-fetch or
  apply the wrong leaf set, but because targeted syncs never GC (§5.6.2), it
  cannot delete unrelated links or checkouts. Destructive reconciliation stays
  reachable only from a full, unfiltered sync. Covered by the `--source`
  selection tests (§6.3).
- **Determinism / caching.** Link order and ids must be a pure function of
  declared config order (§5.2). Because projected files feed `omni_collector`
  hashing (RFC 0001 decision #6), a non-deterministic expansion would silently
  perturb project hashes. Mitigation: expansion is a fold over ordered lists; no
  hashmap iteration in the id/link path.
- **Privilege-floor bypass (security).** Forgetting to run
  `reject_privilege_escalation` on inline child routes would let a bundle grant
  `allow_git`/`allow_omni_config`. Mitigation: apply the floor unconditionally to
  every expanded child (decision #5) and test it.
- **Windows path length.** The `namespaced` path uses the child's authored `id`
  only (the `::` prefix never reaches disk), so meta adds no nesting versus a
  directly-declared source, and no new path-length exposure. Noted for
  completeness.

### 6.6 Pros and cons

| Option                                                                                                                        | Pros                                                                                                                                                                                                                                                                                                                                                        | Cons                                                                                                                                                                                               | Verdict |
| ----------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------- |
| **A. Meta manifest reusing `SourceConfig<ProjectionExtra>` for a nested `sources` list, expanded at sync time (recommended)** | Maximal reuse (whole leaf pipeline, fetch, GC, ledger unchanged); zero workspace-config change; matches the user's "just another source" framing; recursion composes for free via id namespacing; `::`-composed `qualified_id`s make `unlink` and `--source` a clean, config-free prefix match; the `MetaExpand` seam keeps the traversal pure and testable | Adds the "expanded set" invariant several GC sites must respect; introduces the authored-`id` vs `qualified_id` split each site must apply to the right value; hand-written schema grows a `oneOf` | ✅      |
| B. New workspace-config field (e.g. `projection_bundles`) listing bundle sources                                              | Explicit, discoverable in `workspace.omni.yaml`                                                                                                                                                                                                                                                                                                             | New config surface; contradicts the user's framing; duplicates the source model; consumers still edit config per bundle                                                                            | ❌      |
| C. Meta as a first-class new `SourceConfig` variant (`source: meta`)                                                          | Tagged, explicit                                                                                                                                                                                                                                                                                                                                            | The origin is still git/local, so "meta" is a manifest property, not an origin; would fork the source enum for no fetch difference                                                                 | ❌      |
| D. Preprocess/`extends` that textually merges remote projection lists into the workspace config before load                   | Reuses config merge                                                                                                                                                                                                                                                                                                                                         | Fetch-during-config-load breaks offline/deterministic load; no per-child pin/ledger identity; couples config loading to the network                                                                | ❌      |

### 6.7 Prior art and how omni improves on it

| Tool                                  | Their approach                                                                  | What to adopt                                       | What to change for omni                                                                                            |
| ------------------------------------- | ------------------------------------------------------------------------------- | --------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| npm/pnpm meta-packages & catalogs     | A package depends on others; `pnpm-workspace.yaml` catalogs centralize versions | The "one dependency pulls a curated set" ergonomics | Pin per child via the existing lockfile; no registry, just git/local sources                                       |
| Bazel `bazel_dep` + module extensions | A module declares deps; extensions generate repos                               | Bounded, declared expansion with pinning            | Keep it dataless (no rule language); expansion is a pure fold, not code execution                                  |
| Renovate `extends` presets            | Config files reference shared presets, recursively                              | The preset-of-presets mental model                  | Support recursion with a depth cap + ancestor-path cycle detection; pin every level; namespace by id               |
| ESLint/Prettier shareable configs     | `extends`/`presets` merge shared config                                         | Author-owned membership, consumer one-liner         | Meta bundles _sources_ (files-to-link), not merged config; untrusted routes keep the privilege floor               |
| Terraform module composition          | A module calls child modules with pinned sources                                | Per-child pinned source + namespacing               | Namespace via a reserved `::` id delimiter (unauthorable, so teardown is unambiguous); reuse the projection ledger |

The concrete improvement: rather than a bespoke preset/extends mechanism, meta
projections _compose_ with omni's existing source/pin/GC/ledger machinery, so a
bundle inherits pinning, teardown, collision detection, and cross-platform
linking for free, while a strict trust boundary (untrusted routes, no
control-plane authority, contained local children) keeps a remote bundle from
escalating beyond "these files, linked here."

## 7. Decision log

| #   | Decision                                                                                                                                                                                                                                                                                                                                                                                                                                                  | Rationale                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Put the meta capability in the source-owned `projection.omni` manifest, not in `WorkspaceConfiguration`                                                                                                                                                                                                                                                                                                                                                   | Matches the user's framing ("just another source with its own projection config"); zero consumer-facing wire change; a bundle owns its own membership/pins/routes                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| 2   | Model the manifest as a proper **enum** (`Leaf { routes } \| Meta { sources }`), deserialized through a private `deny_unknown_fields` two-optional wire adapter with an exactly-one check                                                                                                                                                                                                                                                                 | The manifest is a true sum type, so the domain type should make both/neither unrepresentable rather than lean on a runtime validator over a two-`Option` struct. A tag-free wire is required (the `{ routes }` leaf form already ships), which rules out a derived `#[serde(tag="kind")]` enum (would force `kind:` onto every existing manifest) and `#[serde(untagged)]` (opaque errors, fights `deny_unknown_fields`). The deserialize-through-intermediate pattern keeps the clean enum, the back-compatible wire, precise errors, and exact unknown-field rejection, mirroring how `Projection` pairs a tagged `Deserialize` with a hand-written `JsonSchema` (`oneOf`). The tag-free wire is the settled choice, not merely an option: the manifest schema is published externally via `omni config schema projection` (`omni-oss/json-schemas`) and omni has no config-migration mechanism, so a `kind:` discriminator would silently break every existing third-party manifest with no upgrade path |
| 3   | Compose the **identity** of an expanded source with a reserved `::` delimiter (`<meta>::<child>`) via compose-after-validate: validate each authored segment with `validate_projection_id`, join with `::`, and never re-validate the composed string. Enforce uniqueness over the expanded `qualified_id`s                                                                                                                                               | The ledger keys links by `qualified_id`; a duplicate would corrupt teardown (RFC 0001 #18). `::` is unauthorable (the validator already rejects `:`, per `rejects_illegal_characters`), so a `qualified_id` can never collide with a hand-authored id, and "`:` is unauthorable" stays literally true because the composed string bypasses author-id validation by construction. This is what makes `unlink`/`--source` clean prefix matches (#7, #11)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| 4   | A workspace source whose manifest resolves to _meta_ may not also carry top-level `routes`; hard error                                                                                                                                                                                                                                                                                                                                                    | There is no 1:1 route set to override; silently ignoring `routes` would let config lie about what it links, breaking `--dry-run`/`status` truthfulness (same principle as RFC 0001 #22)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| 5   | Apply `reject_privilege_escalation` unconditionally to every expanded child's resolved routes (inline or owned)                                                                                                                                                                                                                                                                                                                                           | Routes reachable through a meta manifest are untrusted regardless of where they were written; only a _workspace_ source's own top-level `routes` are trusted to set `allow_*`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| 6   | Allow **bounded recursion** (bundle-of-bundles), guarded by ancestor-path cycle detection (primary) + an overridable depth cap (backstop, `DEFAULT_META_PROJECTION_DEPTH = 16`)                                                                                                                                                                                                                                                                           | Mirrors omni's proven `run-generator` nesting model (`bridge_rpc_runner::DEFAULT_MAX_DEPTH` doc: "static cycle detection is the primary guard; this bound is defense-in-depth"; generator `visited`-set + `max_depth`). Id composition already supports arbitrary depth (RFC 0001 #26), so no new id machinery is needed; **diamonds** are allowed (distinct expanded ids), only true back-edges error; the depth cap catches the same-repo-different-rev case identity cannot. The cap value is deliberately **not** the generator/bridge `64`: each meta descent is a git fetch, so 16 fails a runaway fan-out ~4× sooner while staying far above real nesting (2 or 3); only the mechanism mirrors generators, not the number                                                                                                                                                                                                                                                                            |
| 7   | `unlink <meta-id>` tears down the whole bundle via a **`::`-anchored prefix match** (`qualified_id == arg \|\| qualified_id.starts_with("arg::")`) in `omni_projections::unlink`, the **same predicate** `sync --source` uses (#11)                                                                                                                                                                                                                       | Exact match would strand a bundle's expanded links. Because `::` is unauthorable (#3), the prefix match has no false-positive: only that bundle's machine-composed children begin with `<arg>::`. Ledger-only, no config read, no ledger-format change (#10). Preferred over `/`-prefix (ambiguous with nested author ids, needs a config read), a `--bundle` opt-in, or ledger parentage (breaks #10). Detailed in §5.6.1                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| 8   | Derive `keep_ids`, git-retain refs, and the `--dry-run` planned list from a single `expand()` result                                                                                                                                                                                                                                                                                                                                                      | Centralizes the "must use the expanded set" invariant in one place instead of recomputing it at each GC site, where a miss is silent and destructive                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| 9   | Local children resolve relative to the meta source root and are realpath-contained within it                                                                                                                                                                                                                                                                                                                                                              | Prevents a bundle from projecting arbitrary machine paths; reuses the engine's existing `containment` helpers; symmetric with RFC 0001's source-escape rule                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| 10  | No ledger-format and no lockfile-format change                                                                                                                                                                                                                                                                                                                                                                                                            | Meta contributes only expanded leaves; each leaf's link and pin are recorded exactly as today, so a fresh clone still fully recovers via `sync` (RFC 0001 #30 stays intact)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| 11  | Support **per-child `--source`** in v1 via the same `::`-anchored predicate as `unlink`, with top-level pre-filter + within-bundle traversal pruning, the GC gate left binary (`req.source.is_none()`), and matched-nothing promoted from a silent no-op to a hard error                                                                                                                                                                                  | Targeted syncs already never GC, so per-child inherits that safety with zero GC rework: a pruning bug can only over-fetch or write the wrong leaf set (fixed by re-sync), never delete unrelated links or checkouts. One predicate shared by `sync --source` and `unlink` gives a uniform mental model ("an arg names a source or a bundle subtree"). Pruning preserves the existing "targeted sync touches nothing unrelated" guarantee by not fetching unselected top-level sources                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| 12  | Keep the authored `id` untouched and carry the composed form only in a system-assigned `qualified_id` (on the internal `EffectiveSource`, never deserialized). Identity uses `qualified_id`; the `namespaced` path uses the authored `id`, so `::` never reaches disk. `PlanInput` carries both                                                                                                                                                           | Keeping the path on the authored `id` preserves RFC 0001 #23 ("namespace = id") with no exception and makes a bundled source land where a directly-declared one would. Accepted cost: two bundles shipping a same-named `namespaced` child collide on disk, caught by the whole-run preflight (#14) rather than prevented structurally. A wrong field at any site is a silent bug (path gets `::`, or the ledger collapses two children), so the split is stated explicitly and enforced by `PlanInput` carrying two typed values                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| 13  | Build the traversal, `qualified_id`/`::` composition, `SourceIdentity` cycle detection, depth cap, and selection predicate behind a subsystem-agnostic `MetaExpand` trait; projections are the first (and today only) impl. Route/ledger/GC/path logic stays in the projection impl; tools/generators are explicit future work                                                                                                                            | The fetch/classify I/O behind the trait makes the traversal pure and unit-testable with a fake `classify` (a testability win today, not only future-proofing) and stops projection route logic from entangling the traversal. It does **not** give tools/generators meta for free: they use `SourceConfig<NoExtra>` and lack any owned-manifest-reconfigures-source layer, which must be built before either can bundle. The seam guarantees the traversal/id/cycle/depth/selection are ready when it is                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| 14  | Emit two dedicated, detailed collision errors: (a) two children in one bundle with the same `id` names the bundle and the repeated child id; (b) two bundles' same-named `namespaced` children colliding on a target path names both bundles and the contested path                                                                                                                                                                                       | Meta lets a consumer hit a collision inside a bundle they did not author and cannot open. A generic "duplicate id" or "path conflict" message is unactionable; the bundle id and path are already in hand at the failure site, so naming them is nearly free and directly serves the not-the-author reality meta introduces                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| 15  | House the generic core in a dedicated `omni_meta` crate, generic over the member config type and depending only on `omni_configurations`; the projection `MetaExpand` impl and all route/ledger/GC/path logic stay in `omni_api`/`omni_projections`, and no tool/generator wiring ships                                                                                                                                                                   | A crate boundary makes decision #13's purity a compile-time fact rather than a convention, and isolates the fake-`classify` tests; the workspace already has many small single-purpose crates. Generic-from-day-one is nearly free because `SourceConfig<E>` is already generic, and specializing to projections would undercut the very purity the separate crate exists to enforce. Generality lives in the type parameters and the trait, with exactly one implementation and no speculative second consumer                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| 16  | On `--update`, the re-read manifest is authoritative for what is linked; a dropped child is removed only on a full sync. A targeted `sync --update --source <bundle>` does not remove a dropped child but warns, naming the now-absent child and its lingering links                                                                                                                                                                                      | Removing on a targeted run would break the invariant that targeted syncs never GC (#11), which is what makes them safe to reason about. Leaving it silent would strand a dropped member with no signal. A warning surfaces the drift and points at the full sync that reconciles it                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| 17  | Render human-readable `status` (and `sync`/`--dry-run`) output as a tree grouped by `qualified_id` prefix, one indent per `::` segment, with group headers showing descendant-state rollup counts and the bundle name in **bold** (via `owo-colors`, already used across `omni_cli_core`) to distinguish a synthesized bundle node from a leaf member; the machine-readable API/MCP report stays a flat entry list and the tree lives in the CLI renderer | The grouping is reconstructible from the ledger alone because `::` is unauthorable, so `status` stays ledger-and-filesystem only. Rollup counts stay truthful under mixed states (a worst-state header overstates one failure; a bare header hides the mix) and double as a size signal. Bold group names reuse the existing CLI styling crate, so no new dependency. Keeping the API report flat preserves line-oriented and machine parsing                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |

## 8. Open questions

None. Every design and implementation question that drove earlier drafts is
resolved in the decision log (#1–#17): wire tagging (#2), depth value (#6), the
`id`/`qualified_id` split (#3/#12), per-child `--source` (#11), the `MetaExpand`
seam (#13), collision errors (#14), the `omni_meta` crate and its generic shape
(#15), `--update` dropped-member handling (#16), and `status` tree rendering
(#17).

### Future work (not blocking, out of scope)

- **Meta for `tools`/`generators`.** `omni_meta` is generic over the member
  config type (#15), so a future tool/generator bundle would supply its own
  `classify` implementation. It is gated on those subsystems first gaining a
  shipped-manifest-reconfigures-source layer, which does not exist today (both
  read `generator.omni.*` / `tool.omni.*` as the leaf config itself, over
  `SourceConfig<NoExtra>`; see §5.7). Not built here.
