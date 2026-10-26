# RFC 0014: Source packs, identity, and the source-config profile

- **Status:** Implemented
- **Owner:** TBD
- **Date:** 2026-09-16
- **Affected crates/files:** `omni_configurations` (`source`, `workspace_configuration`, `owned_projection`, `validators`, new `pack`), `omni_input_schema` (the `InputProfile` precedent the profile pattern mirrors), `omni_projection_configurations` (`ProjectionExtra`), `omni_meta` (`MetaExpand::member_id` → `declared_id`, the shared traversal), `omni_api` (`operations::{generator, tool, projection, remote_source}`), `omni_remote_source_contributors`, `omni_context` (effective-source read sites), `omni-oss/json-schemas`
- **Prior art referenced:** Nix flakes, VS Code Extension Packs, Nx plugins, Backstage plugins, ESLint shareable configs / Renovate presets, Cargo (`package =` rename), Bazel bzlmod (apparent vs. canonical repo names), Go modules (import-path identity), npm (scopes + install aliases)

## 1. Summary

This RFC does three things, in dependency order, in one document:

1. **A `SourceConfigProfile` trait.** Today `SourceConfig<E>` carries a single
   `extra` type `E` flattened identically into both the `local` and `git`
   variants. This RFC generalizes it to a per-variant profile. A trait carries a
   shared `BaseExtra` (applied to every variant) plus a `LocalExtra`, `GitExtra`,
   and a serde-hidden `RegistryExtra` stub, mirroring the shipped `InputProfile`
   pattern (`omni_input_schema`). This makes an identifier's shape differ by
   source kind, which is what the identity model below needs, and readies the type
   for a future registry source with no later migration.
2. **A two-identifier model for source-named units** (projections and packs). A
   consumer-assigned `id` (required on `git`/`local`, where a source's own name is
   only knowable after fetch) stays the pre-fetch key that `omni_meta` composes
   qualified ids from and that the projection ledger keys on. An author-declared
   canonical `name` (scoped, `@org/name`) is added to the source manifest as
   the source's own identity, its display label, and the coordinate a future
   registry resolves by. On the registry variant (stubbed here) the `id` becomes
   optional and defaults to that name. This applies to both projection
   manifests and pack manifests. Projections gain a canonical name they never had,
   with zero ledger churn because the ledger keeps keying on the consumer `id`.
3. **Source packs.** A single top-level `packs:` list. One entry names one source
   (git or local) that must carry a root `pack.omni.*` manifest. That manifest is a
   dedicated, ambient-free type carrying its own `generators`/`tools`/`projections` and,
   recursively, the `packs` it builds on, plus `name`/`version`/`description`. omni
   expands the manifest and fans its declared sources into the three subsystems'
   effective source lists. Pack-of-packs recursion reuses the `omni_meta` traversal
   RFC 0010 already uses. No new fetch, lockfile, or GC machinery is introduced.

The mechanism is a thin config-level aggregation on top of the shared content
store from RFC 0013 (which already fetches an identical `{uri, rev}` exactly once)
and the source-owned projection manifests from RFC 0010. The profile refactor
lands first and migrates every existing consumer onto it; the identity model and
packs build on that.

## 2. Motivation / Problem

### 2.1 The triplication problem

A pack author's repo already works as a bundle at the discovery layer: generator
discovery globs the whole checkout for `generator.omni.*`, tool discovery globs
for `tool.omni.*`, and projection sync reads an owned `projection.omni.*` at the
source root. The only friction is the consumer's `workspace.omni.yaml`, which
must repeat the source three times and, for larger ecosystems, the fact that
there is no way for one published unit to build on another (an org's
`agent-kit` cannot say "I am `base-kit` plus these extras"). The triplication:

```yaml
generators:
    - source: git
      uri: https://github.com/org/agent-kit.git
      rev: v1.2.3
tools:
    - source: git
      uri: https://github.com/org/agent-kit.git
      rev: v1.2.3
projections:
    - source: git
      uri: https://github.com/org/agent-kit.git
      rev: v1.2.3
      id: agent-kit # consumer must invent an id
      routes: [...] # consumer must know the author's layout
```

This is verbose, error-prone (three `rev`s that must be kept in lockstep), and
leaks the author's internal layout to the consumer (the `routes`). The desired
outcome is one entry:

```yaml
packs:
    - source: git
      uri: https://github.com/org/agent-kit.git
      rev: v1.2.3
      id: agent-kit
```

### 2.2 The identity gap: source-named units cannot name themselves

Designing packs revealed that omni's subsystems fall into two identity classes:

- **Content-named collections: generators and tools.** A source is a bag
  of self-named items: `GeneratorConfiguration.name` / `ToolConfiguration.name`
  are the workspace-unique identities (a duplicate is a hard error, e.g.
  `omni_generator`'s `"there is already a generator with the name '{name}'"`), and
  the source is `SourceConfig<NoExtra>`, so it needs no name of its own.
- **Source-named units: projections and packs.** The source is the
  unit; there is no inner named item, so identity must attach to the source.

Projections attach it as `ProjectionExtra.id`, assigned by the consumer at the
declaration site. That works for placement, but it means a projection source
cannot name itself. The author has no canonical identity for the thing they
publish. There is no author branding in `projection status`, no shared handle
across consumers (two workspaces may pick different `id`s for the same source),
and, looking ahead, no name for a future registry to enforce and resolve by.
Packs would introduce the same class of unit; giving packs a canonical name but
leaving projections without one would be an arbitrary asymmetry between two
structurally identical subsystems.

### 2.3 The per-variant tension a registry forces

The natural fix, "let the author declare a `name` and default the consumer's
handle to it," collides with a hard, verified constraint. `omni_meta::member_id`
is synchronous, returns a borrowed `&str`, and runs before `classify`
materializes a source, because the composed `parent::child` qualified id is used
to reject duplicate siblings and to prune unselected subtrees without fetching
them (`selection_prunes_unrelated_subtrees`). A manifest-owned `name` lives
inside the not-yet-fetched checkout, so it cannot be the pre-fetch key on a
`git`/`local` source.

Comparable ecosystems resolve this with a two-layer identity. A mutable,
locally-chosen or registry-enforced coordinate sits over an immutable content
pin. In a registry-less (git/local) world the pre-fetch coordinate must be the
consumer-supplied handle; a registry supplies the name pre-fetch and enforces its
uniqueness at publish time, which resolves the tension. Because the `id`'s
required-ness therefore differs by source kind (required on git/local, optional
on registry), the identifier cannot sit in a single shared `extra`. It must be a
per-variant field. That is the concrete reason the `SourceConfig` type is
generalized to a profile now rather than later. It is the type-level home for a
field whose shape depends on the variant, and building it once avoids migrating a
type used by four subsystems when the registry lands.

### 2.4 Who is affected

- **Pack authors** distributing an agent/skill together with the generators,
  tools, and projected assets it depends on, as one installable unit with one
  identity, and composing packs from other packs.
- **Projection authors** who want their published source to carry its own name.
- **Consumers** adopting such a source without re-declaring it three times or
  inventing an identity for someone else's content.

### 2.5 Why now

RFC 0013 made every subsystem materialize through one shared, commit-keyed
content store, so registering a source three times already fetches it once; the
remaining cost of triplication is config ergonomics and hand-written routes.
RFC 0010 already lets a source own its projection routes via a manifest, and its
`omni_meta` traversal already composes qualified ids, detects cycles, and prunes
subtrees. The pieces a pack needs exist; this RFC combines them into one config
entry and closes the identity gap for both source-named
subsystems on a type that is registry-ready.

## 3. Goals / Non-goals

### Goals

- Generalize `SourceConfig<E>` into a `SourceConfigProfile` trait with a shared
  `BaseExtra` and per-variant `LocalExtra`/`GitExtra`/`RegistryExtra`, mirroring
  the shipped `InputProfile` pattern, and migrate all existing consumers
  (generators, tools, projections) onto it as the first milestone.
- Give source-named units (projections and packs) an author-declared canonical
  `name` in their manifest, while keeping the consumer-assigned `id` as the
  required pre-fetch key on git/local and as the projection ledger key. Adding the
  name must be non-breaking and churn-free for existing ledgers.
- Land the `Registry` source variant + `RegistryExtra` as a serde-hidden,
  documented stub now (inert on the wire and schema), so the future registry is a
  pure addition rather than a migration.
- A single `packs:` entry registers one source into the generator, tool, and
  projection subsystems at once. A pack is always an explicit, manifest-declared
  artifact; there is no auto-detection.
- Projections from a pack are opt-in on the author side (the manifest declares
  them); consumer `provides` narrows the subsystems a pack contributes and cascades
  to its sub-packs.
- Pack-of-packs in v1, reusing the `omni_meta` traversal rather than a second
  recursion mechanism. Pack-contributed generator/tool names are namespaced by
  the qualified id so they cannot collide with the workspace's own or each
  other's.
- No duplicate fetching, no new lockfile, no new GC path.

### Non-goals

- **Designing the registry itself.** This RFC only makes the type and both
  identity models registry-ready and lands an inert stub. Name-to-pin resolution,
  registry fetch, and registry GC are a future RFC.
- **A fourth runtime subsystem.** A pack is an aggregation of the three existing
  subsystems; there is no new graph, hasher, or executor.
- **Cross-pack dependency resolution / semver solving.** Every pack (and sub-pack)
  is pinned by `rev`. The manifest's `version` is metadata only, neither a
  constraint nor a diamond-unification key.
- **Ambient/workspace-global config in a pack.** A `pack.omni.*` carries no `env`,
  `capabilities`, `enable_experimental`, `ignore`, `ui`, or `projects`.
- **Namespacing generator/tool _item_ names in a plain (non-pack) source**, or
  changing routing strategies, trust floors, or hashing.

## 4. Background: current mechanisms

Grounded in the code as it exists today.

### 4.1 Sources are `SourceConfig<E>`, a single flattened `extra` per variant

`omni_configurations::SourceConfig<E>` (`crates/omni_configurations/src/source.rs`)
is a `#[serde(tag = "source")]` `Local`/`Git` enum. **One** `extra: E` is
`#[serde(flatten)]`ed identically into each variant, defaulting to `NoExtra`:

```rust
pub enum SourceConfig<E = NoExtra> {
    Local(LocalSource<E>),
    Git(GitSource<E>),
}
impl<E> SourceConfig<E> {
    pub fn extra(&self) -> &E { /* match Local/Git → &.extra */ }
}
pub struct LocalSource<E = NoExtra> { pub path: SingleOrMany<String>, #[serde(flatten)] pub extra: E }
pub struct GitSource<E = NoExtra>   { pub uri: Url, pub rev: String,   #[serde(flatten)] pub extra: E }
```

- `GeneratorSourceConfiguration = SourceConfig<NoExtra>`
- `ToolSourceConfiguration = SourceConfig<NoExtra>`
- `ProjectionSourceConfiguration = SourceConfig<ProjectionExtra>` where
  `ProjectionExtra { id, routes }` adds the sibling `id`/`routes` keys.

The list validators (`crates/omni_configurations/src/validators.rs`) reject
duplicate git URIs within a list (`SourcesValidator<NoExtra, _>`), and for
projections additionally reject duplicate `id`s (`ProjectionSourcesValidator`).
`SourceNameValidator` + `SOURCE_NAME_REGEX` (`^(?:@[a-zA-Z0-9._-]+/)?[a-zA-Z0-9._-]+$`)
already exist in that file but are currently `#[allow(unused)]`, a
registry-shaped, scoped-name validator waiting for a first consumer.

### 4.2 The `InputProfile` precedent: a per-variant profile over flatten

`omni_input_schema::InputProfile` (`crates/omni_input_schema/src/profile.rs`) is a
`Default + Sized` marker trait whose associated types (`Base`, `Boolean`, `String`,
`Numeric`, `Array`, `Object`, `AllowedValueBase`) are each bounded
`for<'de> Deserialize<'de> + Serialize + JsonSchema + Debug + Clone + PartialEq + Default + Send + Sync`,
plus a few methods that operate over those types (`is_remember`, presentation
hints). Its variant structs (`crates/omni_input_schema/src/input.rs`) prove the
serde fact this RFC depends on. A `#[serde(deny_unknown_fields)]` struct with
multiple `#[serde(flatten)]` fields (`base: BaseInput`, `base_extra: E::Base`,
`string_extra: E::String`) and `#[serde(bound(deserialize = "", serialize = ""))]`
still rejects unknown fields at omni's pinned serde version. The
`unit_profile_rejects_unknown_field` test (RFC 0002) is the guardrail. This is the
exact mechanism `SourceConfigProfile` reuses.

### 4.3 Discovery globs the whole checkout

Each subsystem, given a materialized source root, scans it for its manifests:
generators via `omni_generator::discover(&root, &["**"], &sys)`, tools via the
mirror path, projections via `discover_owned_manifest(sys, &root)` then route
classification. A single checkout already yields all three independently; the list
an entry lives in only decides which discovery runs against its checkout.

### 4.4 One shared, commit-keyed store (RFC 0013)

`open_source_store(ctx, sys)` returns the one `RemoteSourceManager` over
`.omni/sources/`. `store.materialize(&RemoteSource::Git { uri, rev })` clones into
a commit-keyed, content-addressed directory and returns `MaterializedSource { root,
pin }`. Two registrations of the same `{uri, rev}` resolve to the same commit and
the same on-disk checkout, fetched once. Pins live in one committed
`.omni/sources/lock.json`. GC is a union-of-refs reconciliation performed only by
`omni remote-sources install`.

### 4.5 Per-subsystem contributors (RFC 0013)

`omni_remote_source_contributors` holds `GeneratorRemoteContributor`,
`ToolRemoteContributor`, and `ProjectionRemoteContributor`, each built from its
injected source list; `handle_remote_sources_install` discovers subsystems from
`WorkspaceConfiguration.{generators, tools, projections}`, drives the contributors
against the shared store, and writes each `refs/<id>.json`.

### 4.6 Source-owned projection manifests (RFC 0010)

`OwnedProjectionConfiguration` (`crates/omni_configurations/src/owned_projection.rs`)
is `Leaf { routes }` XOR `Meta { sources }`, deserialized through a private
`#[serde(deny_unknown_fields)]` `RawManifest` (two `Option` fields) + `TryFrom` that
enforces exactly-one, and re-serialized tag-free. It carries no name / version /
description today. Trust is narrow: only a depth-0 source whose `routes` come from
the workspace config override is trusted (`trusted = depth == 0 &&
workspace_routes.is_some()`, `crates/omni_api/src/operations/projection.rs`). Every
other route set (an owned manifest at any depth, or a bundled child's inline
routes) runs through `reject_privilege_escalation`, which forbids only the two
control-plane escape hatches `allow_omni_config`/`allow_git`; it does not otherwise
restrict which routes a source projects. A projection source that resolves to no
routes errors (`must project at least one route`).

### 4.7 The generic meta traversal (`omni_meta`)

`omni_meta::expand` (`crates/omni_meta/src/expand.rs`) is a subsystem-agnostic
pre-order DFS over source lists, already used by projections for meta sources. All
I/O is behind `MetaExpand`:

```rust
pub trait MetaExpand {
    type Extra;   // the extra family on each SourceConfig
    type Leaf;    // the payload a node contributes
    type Error: From<omni_meta::Error>;
    async fn classify(&self, src: &SourceConfig<Self::Extra>, qualified_id: &str,
                      parent_root: &Path, depth: usize)
        -> Result<Materialized<Self::Leaf, Self::Extra>, Self::Error>;
    fn member_id<'a>(&self, src: &'a SourceConfig<Self::Extra>) -> &'a str;
}

pub struct Node<Leaf, Extra> {          // generalized as a prerequisite for this RFC
    pub leaf: Option<Leaf>,
    pub children: Vec<SourceConfig<Extra>>,
}
// `Node::leaf(x)`, `Node::meta(children)`, `Node::both(x, children)`.
```

`member_id` is synchronous and returns a borrowed `&str` (for projections, today,
`src.extra().id`); the composed `parent::child` qualified id is produced _before_
`classify` runs. The driver provides, unit-tested with a fake expander:
`SourceIdentity`-based cycle detection, a depth cap (`DEFAULT_META_PROJECTION_DEPTH
= 16`), qualified-id composition, duplicate-sibling-id rejection, and subtree
selection that never fetches unselected subtrees. The ledger keys projected links
by that qualified id (`handle_projection_sync` retains via `l.source_id !=
prep.qualified_id`).

The `Node` struct (formerly a `Leaf`/`Meta` enum) already landed in a
behaviour-preserving refactor; projections only ever emit leaf-only or meta-only
nodes, so their expansion is unchanged. Packs (§5) are the first user of both-and.

## 5. Proposed design

The design lands in three milestones, in strict dependency order. M1 is the
`SourceConfigProfile` trait plus consumer migration; M2 is the identity model on
both source-named subsystems (projection `name`, pack `name`, registry stub); M3
is source packs on top.

### 5.1 M1: the `SourceConfigProfile` trait

`SourceConfig` becomes generic over a **profile** rather than a single extra. The
profile assigns each source variant its own extra type and one `BaseExtra` shared
by every variant:

```rust
// omni_configurations::source
pub trait SourceConfigProfile: Default + Sized {
    /// Applied to *every* variant. The home for fields that are identical across
    /// source kinds (e.g. a pack's `provides`, a projection's inline `routes`).
    /// Optional in practice: profiles that need nothing here use `()`.
    type BaseExtra:     /* Deserialize + Serialize + JsonSchema + Debug + Clone + PartialEq + Default + Send + Sync */;
    type LocalExtra:    /* …same bounds… */;
    type GitExtra:      /* …same bounds… */;
    /// Registry variant extra. Present in the type today but the variant is
    /// serde-hidden (§5.1.1); this is where `id` becomes optional (§5.2).
    type RegistryExtra: /* …same bounds… */;

    /// The declaration-site identity handle, if this source kind carries one.
    /// Uniform accessor over the per-variant extras (git/local carry a required
    /// `id`; the registry variant an optional one). Generators/tools return `None`.
    fn declared_id(source: &SourceConfig<Self>) -> Option<&str>;
}

pub enum SourceConfig<P: SourceConfigProfile = ()> {
    Local(LocalSource<P>),
    Git(GitSource<P>),
    #[serde(skip)] // §5.1.1: inert stub for the future registry
    Registry(RegistrySource<P>),
}

// Each variant multi-flattens the shared base + its own extra, exactly like
// `Input<E>` (§4.2): the serde `bound` + `deny_unknown_fields` pair, plus the
// matching `schemars(bound)` every `*Input` struct carries (without it, schema
// generation over the profiled generics does not compile). `deny_unknown_fields`
// survives the multi-flatten.
#[serde(bound(deserialize = "", serialize = ""), deny_unknown_fields)]
#[schemars(bound(deserialize = "", serialize = ""))]
pub struct LocalSource<P: SourceConfigProfile> {
    pub path: SingleOrMany<String>,
    #[serde(flatten)] pub base: P::BaseExtra,
    #[serde(flatten)] pub extra: P::LocalExtra,
}
#[serde(bound(deserialize = "", serialize = ""), deny_unknown_fields)]
#[schemars(bound(deserialize = "", serialize = ""))]
pub struct GitSource<P: SourceConfigProfile> {
    pub uri: Url,
    pub rev: String,
    #[serde(flatten)] pub base: P::BaseExtra,
    #[serde(flatten)] pub extra: P::GitExtra,
}

impl<P: SourceConfigProfile> SourceConfig<P> {
    pub fn base(&self) -> &P::BaseExtra { /* match all variants → &.base */ }
    pub fn declared_id(&self) -> Option<&str> { P::declared_id(self) }
}
```

The null profile is `()`: `impl SourceConfigProfile for ()` sets every associated
type to `()` and returns `None` from `declared_id`, exactly as the shipped
`impl InputProfile for ()` (§4.2). It is the migration target for generators and
tools, replacing `NoExtra` while preserving the exact wire shape (flattening `()`
contributes no fields).

There is one non-mechanical consequence. Because the per-variant extras have
different types, there is no uniform `extra()` accessor anymore. Cross-variant
reads go through `base()`; identity goes through `declared_id()`.
`omni_meta::member_id` (which reads `src.extra().id` for projections) is renamed
and re-pointed to `declared_id`, returning the per-variant `id`. This is a small
touch on the crate whose `Node` was just refactored; the traversal logic is
otherwise unchanged.

#### 5.1.1 The registry variant is a serde-hidden stub

The `Registry` variant, `RegistrySource<P>` (carrying `name`/`version` as struct
fields, exactly as `git` carries `uri`/`rev`), and `RegistryExtra` are added to
the types now, but the variant is `#[serde(skip)]` (and `#[schemars(skip)]`) so
it never appears on the wire or in the published schema and is never constructed.
It exists purely so the future registry is a pure addition: the per-variant extras
already give `id` a place to be optional on registry while required on git/local,
and the locator fields already follow the `git` pattern. A doc comment on the
variant states this explicitly, so a future contributor knows why an unused,
hidden variant is in the enum.

### 5.2 M2: the two-identifier model for source-named units

For projections and packs, identity is a two-layer coordinate over the content pin
(the resolved `{uri, rev}` maps to a commit, already the store's key):

- **Declaration-site `id`.** Lives in the per-variant extra. Required on
  `git`/`local` (`GitExtra`/`LocalExtra` carry `id: String`), because a source's
  canonical name is inside the not-yet-fetched checkout and `member_id` is
  pre-fetch and synchronous (§2.3). It stays the qualified-id segment, the
  duplicate-detection key, and (for projections) the ledger key. On the hidden
  `RegistryExtra` it is `id: Option<String>`, defaulting to the registry `name`.
- **Canonical `name`.** Author-declared in the source manifest (`pack.omni.*` or
  `projection.omni.*`), scoped and validated by the now-consumed `SourceNameValidator`
  (`@org/name`). It is the source's own identity, its display label in `omni pack`
  / `omni projection status`, and the coordinate a future registry resolves and
  enforces uniqueness on. It is not a qualified-id segment and not the
  ledger key, so it can be read post-fetch and can change without re-keying
  anything. `version`/`description` accompany it as metadata (`version` per §5.6 is
  not a constraint).

This is the Cargo/Bazel shape: an author-owned canonical name plus a
locally-chosen handle. Under the (future) registry, the consumer writes `source:
registry`, `name`, `version` at the declaration site, so the name is known
pre-fetch and enforced unique, at which point `id` defaults to it and the handle
reverts to a pure alias (npm `myalias@npm:real`, Cargo `package =`).

### 5.3 M1 continued: consumer profiles

Projections and packs each define a marker profile; generators and tools carry
nothing extra, so they reuse the null profile `()` directly (`SourceConfig<()>`,
formerly `SourceConfig<NoExtra>`):

| Subsystem   | `BaseExtra`               | `GitExtra` / `LocalExtra` | `RegistryExtra` (stub)   | `declared_id` |
| ----------- | ------------------------- | ------------------------- | ------------------------ | ------------- |
| generators  | `()`                      | `()`                      | `()`                     | `None`        |
| tools       | `()`                      | `()`                      | `()`                     | `None`        |
| projections | `{ routes: Option<…> }`   | `{ id: String }`          | `{ id: Option<String> }` | `Some(id)`    |
| packs       | `{ provides: Option<…> }` | `{ id: String }`          | `{ id: Option<String> }` | `Some(id)`    |

`provides` and the inline projection `routes` override are cross-variant, so they
sit in `BaseExtra`; `id` is the only field whose shape depends on the variant, so
it is the only per-variant field. Generators and tools carry nothing and are
identity-less (they are content-named collections, §2.2). The projection list
validator moves from reading `source.extra().id` to `source.declared_id()`; its
duplicate-`id` and duplicate-URI checks are otherwise unchanged, preserving
existing test semantics.

### 5.4 M2 continued: projection naming, churn-free

`OwnedProjectionConfiguration` gains `name`/`version`/`description`. Structurally it
stops being a bare `Leaf`/`Meta` enum and becomes a metadata-carrying value with an
XOR body, deserialized through the same private `RawManifest` + `TryFrom` pattern it
already uses (now `{ name?, version?, description?, routes?, sources? }`, exactly-one
of `routes`/`sources`, `deny_unknown_fields`, tag-free). A pre-existing
`{routes:[...]}` manifest parses unchanged because all new keys are optional. This
is additive, with no discriminator, honoring RFC 0010 #2 (omni has no
config-migration mechanism and publishes the schema externally).

The ledger is untouched. `handle_projection_sync` keeps keying links
by the `qualified_id` composed from the consumer `id` (`member_id`/`declared_id`).
The author `name` is metadata, registry coordinate, and display label; it is never
a ledger key, so adding it re-keys nothing and orphans no links. The "severe
ledger-churn risk" only exists if `name` were made the key, which this design
structurally avoids. Consumer `id` remains required on git/local; on the future
registry it defaults to `name`.

### 5.5 M3: the `packs` config

A new optional top-level list in `WorkspaceConfiguration`, mirroring the three
source lists:

```yaml
packs:
    - source: git
      uri: https://github.com/org/agent-kit.git
      rev: v1.2.3
      id: agent-kit
    - source: local
      path: ./packs/house-style
      id: house-style
```

A pack entry is a `SourceConfig<PackProfile>`. Its `BaseExtra` carries the optional
consumer-side subsystem gate; its per-variant extra carries the required `id`:

```rust
// omni_configurations::pack
#[derive(Default)] pub struct PackProfile;

pub struct PackBase {
    /// Consumer-side subsystem gate. When omitted, the pack contributes every
    /// subsystem its manifest declares (§5.9). Narrowing only; a consumer can never
    /// widen what a pack offers. Cascades to sub-packs (§5.9).
    pub provides: Option<Vec<PackSubsystem>>, // {Generators, Tools, Projections}
}
pub struct PackId { pub id: String }         // GitExtra = LocalExtra
pub struct PackRegistryId { pub id: Option<String> } // RegistryExtra (stub)

impl SourceConfigProfile for PackProfile {
    type BaseExtra = PackBase;
    type LocalExtra = PackId;
    type GitExtra = PackId;
    type RegistryExtra = PackRegistryId;
    fn declared_id(s: &SourceConfig<Self>) -> Option<&str> { /* per-variant id */ }
}
pub type PackSourceConfiguration = SourceConfig<PackProfile>;
```

`validate_pack_sources` (mirroring `ProjectionSourcesValidator`) rejects duplicate
git URIs **and** duplicate `id`s within the list; graph-wide `id` uniqueness beyond
the list is guaranteed by `omni_meta` qualified-id composition (§5.7).

### 5.6 M3: the required `pack.omni.*` manifest (ambient-free, both-and)

Every pack must carry one manifest, at the source root only, discovered like
RFC 0010's owned projection manifest (`pack.omni.{yaml,yml,json,toml}`). It is its
own type, the subsystem-source slice of a workspace plus identity and metadata, and
nothing ambient:

```rust
// omni_configurations::pack
pub struct PackManifest {
    /// Author-declared canonical identity (§5.2): scoped, `SourceNameValidator`.
    /// The display label and future registry coordinate; NOT the qualified-id
    /// segment (that is the declaration-site `id`, known pre-fetch).
    pub name: String,
    pub version: Option<String>,     // metadata only (see below); NOT a constraint
    pub description: Option<String>,

    // Any combination may be populated; a both-and pack contributes its own
    // sources AND expands its sub-packs (§5.7).
    pub generators: Vec<GeneratorSourceConfiguration>,
    pub tools: Vec<ToolSourceConfiguration>,
    pub projections: Vec<ProjectionSourceConfiguration>,
    pub packs: Vec<PackSourceConfiguration>,
}

// Deserialized through a private, all-optional `#[serde(deny_unknown_fields)]`
// `RawManifest` (the ambient-field security boundary, §6.5; NOT `#[serde(flatten)]`,
// which would silently disable it) + `TryFrom` that fills defaults, rebases `local`
// paths onto the pack root, and rejects a manifest with no `name` or with all four
// source lists empty.
```

```yaml
# pack.omni.yaml, required at the pack's source root
name: "@org/agent-kit" # canonical author identity; the operational handle is the consumer `id` (§5.5)
version: 1.0.0
description: House agent, its generators, tools, and projected skills.

generators:
    - source: local
      path: ./gen
tools:
    - source: local
      path: ./tools
projections:
    - source: local
      path: ./skills
      id: skills
      routes: [{ strategy: namespaced }]
```

Rules:

- **Required, root-only, no auto-detection.** A `packs:` entry whose materialized
  root has no `pack.omni.*` is a hard error, reported early. This keeps discovery
  unambiguous (a pack repo that is itself a workspace, or ships dev-only
  `generator.omni.*`, is never scanned by accident) and gives every pack an explicit
  identity. Manifest-less sources still work via the existing lists unchanged.
- **No ambient fields (hard error).** `PackManifest` omits `projects`, `env`,
  `capabilities`, `enable_experimental`, `ignore`, `ui`; `deny_unknown_fields` on
  `RawManifest` turns any attempt to set them into a parse error, a security
  boundary by construction (§6.5).
- **Pack-root-relative paths.** Paths may not escape the checkout (`..`/absolute
  rejected), preserving RFC 0013's content-addressed, disposable-checkout invariant;
  route-target containment stays under the RFC 0010 floor.
- **`version` is metadata only.** omni pins by `rev` and does no version solving.
- **A pack is a focused workspace, not leaf-XOR-meta.** A manifest may declare its
  own subsystem sources and compose sub-`packs` in one node (the both-and
  `omni_meta::Node`, §5.7), just as `workspace.omni.yaml` does.

### 5.7 M3: pack-of-packs via `omni_meta`

Pack expansion is one `omni_meta::expand` call with `Profile = PackProfile`
and a leaf payload of a node's three subsystem source lists plus its materialized
root. A both-and node contributes its own leaf **and** recurses its children:

```rust
struct PackLeaf { root: PathBuf, generators: Vec<…>, tools: Vec<…>, projections: Vec<…> }

impl MetaExpand for PackMetaExpand<'_, TSys> {
    type Profile = PackProfile;
    type Leaf = PackLeaf;
    type Error = eyre::Report;
    async fn classify(&self, src, qualified_id, parent_root, depth) -> … {
        // materialize via the shared store; require pack.omni.* at root (hard error);
        // emit Node::both(PackLeaf{…}, manifest.packs) when the manifest declares
        // both its own sources and sub-packs; Node::leaf / Node::meta otherwise.
    }
    fn declared_id<'a>(&self, src) -> &'a str { /* per-variant id */ }
}
```

Cycle detection, the depth cap, qualified-id composition, duplicate-sibling
rejection, and subtree selection all come from the shared driver unchanged. A
both-and node is pushed onto the ancestor path before its children recurse, so
cycle detection covers it. Diamonds are allowed (distinct qualified ids); only true
back-edges error.

### 5.8 M3: fan-out into the effective subsystem lists

After `expand`, the driver merges each contributing node's sources into the three
inputs every consumer already reads, in one place:

```rust
fn effective_generator_sources(&self)  -> Vec<GeneratorSource>;  // explicit + pack-contributed
fn effective_tool_sources(&self)       -> Vec<ToolSource>;
fn effective_projection_sources(&self) -> Vec<ProjectionSource>;
```

| Subsystem   | Contribution (subject to `provides`, §5.9)                                                                                       | Gate                            |
| ----------- | -------------------------------------------------------------------------------------------------------------------------------- | ------------------------------- |
| generators  | the node's generator sources, discovered at the materialized `root`; **item names namespaced by the pack's qualified id** (§5.9) | manifest declares `generators`  |
| tools       | the node's tool sources, discovered at `root`; item names namespaced likewise                                                    | manifest declares `tools`       |
| projections | the node's projection sources, fed into the existing projection `omni_meta` expansion, id-prefixed with the pack's qualified id  | manifest declares `projections` |

The four read sites (`get_generators`, tool discovery, `projection sync`, and the
`install` contributor assembly) read these effective inputs instead of
`WorkspaceConfiguration.{generators, tools, projections}` directly. This is **not** a
config-load desugaring. Expansion is where the network happens, so config load stays
offline and the parse-time duplicate-URI validators still see only the authored
lists. Because the RFC 0013 store dedups by commit, a pack graph is materialized
once regardless of how many subsystems reference it; refs are recorded under whichever
subsystem consumes each contribution, so union GC needs no new namespace.

### 5.9 M3: `provides`, projection opt-in, and name collisions

**Projections are author-opt-in.** Generators and tools are pull-only (run only when
invoked), so contributing them is safe. Projections write into the consumer's tree,
so a pack contributes projections iff its manifest declares a `projections:`
list. A pack that omits the key contributes no projections; a declared-but-empty one
is an author bug and keeps the existing "must project at least one route" error.

**`provides` narrows and cascades.** The consumer's `provides` on a `packs:` entry is
a ceiling on the whole subtree rooted at that entry. It gates the pack's own contributions
and cascades to its sub-packs. A consumer who writes `provides: [generators]`
means "generators only, and nothing this pack transitively pulls in should write to
my workspace either," which is the safe reading for the projection-write concern.
`provides` narrows only; it can never widen what a pack offers.

**Generator/tool name collisions are resolved by qualified-id namespacing.** Because
generator and tool item names are workspace-globally-unique (a duplicate is a hard
error, §2.2), a pack shipping a generator named `component` would otherwise fail any
workspace that already has one, or any second pack that ships the same name, with no
override. Pack-contributed generator/tool item names are therefore namespaced by
the pack's `omni_meta` qualified id (`agent-kit::house-style:component`), globally
unique by construction (the Bazel/Go model), consistent with how the projection contribution
already prefixes qualified ids, and collision-free even in a diamond. This also
removes any explicit-vs-pack precedence question. A pack generator is never a bare
`component`, so it cannot collide with a workspace-declared one. (An ergonomic
"invoke unqualified when unambiguous" shorthand, Nx-style, is left to §7.)

### 5.10 CLI / API

Packs materialize through the existing store, so `omni remote-sources install`
installs them once the effective-source layer feeds the contributors. Three
read-only additions reuse the `omni_meta` qualified-id selection:

```
omni pack list                 # top-level packs + resolved id / name / version
omni pack tree                 # the expanded pack-of-packs graph (like projection status)
omni pack info <qualified-id>  # one pack/subtree: source, pin, subsystems it provides
```

These are pure config/manifest readers over the same expansion; `select` prunes to
the requested subtree. No `omni pack install` is added.

## 6. Mandatory analysis

### 6.1 Existing mechanisms (reuse vs. extend vs. add)

| Concern                | Mechanism reused                                                   | Change                                                                                                                                                              |
| ---------------------- | ------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Source wire shape      | `SourceConfig<E>` + the `Input<E>` multi-flatten profile precedent | Generalize to `SourceConfigProfile` (per-variant extras + shared `BaseExtra`); `()` is the null profile (mirroring `impl InputProfile for ()`), replacing `NoExtra` |
| Scoped-name validation | `SourceNameValidator` / `SOURCE_NAME_REGEX` (present, unused)      | Consume it for the pack/projection canonical `name`                                                                                                                 |
| Projection manifest    | `OwnedProjectionConfiguration` `RawManifest` + `TryFrom`           | Add optional `name`/`version`/`description`; keep tag-free XOR body; ledger key unchanged                                                                           |
| Fetch / dedup / pins   | RFC 0013 shared store                                              | None; packs and sub-packs go through it unchanged                                                                                                                   |
| Recursion              | `omni_meta::expand` (cycle, depth, qualified ids, select)          | New `PackMetaExpand`; `member_id` → `declared_id`; `Node` both-and already landed                                                                                   |
| Install / GC           | RFC 0013 contributors + union GC                                   | Contributors read effective (expanded) lists                                                                                                                        |

The design adds no parallel fetch, lockfile, ref namespace, graph, or recursion
engine. New artifacts: the `SourceConfigProfile` trait + the four consumer profiles,
the serde-hidden registry stub, the projection `name` fields, the `PackProfile` +
`pack.omni.*` manifest, `PackMetaExpand`, and the three effective-source helpers.

### 6.2 Maintenance cost

- **Foundational-type reach.** `SourceConfig` is used by three subsystems today
  (generators, tools, projections), plus `member_id`, every validator, and schema
  generation. M1 touches all of them at once; packs, the fourth consumer, is built
  natively on the profile in M3. This is the deliberate cost of choosing the
  type-level model now (decision A1) over a runtime validator; it is bounded by
  mirroring `Input<E>` verbatim and by landing M1 as its own reviewable milestone.
- **A `RegistryExtra`/`Registry` stub that no code exercises.** Carries a small
  ongoing "why is this here" cost, mitigated by the mandatory doc comment (§5.1.1)
  and by the stub being what makes the registry a pure addition.
- **Two identity models kept in lockstep.** Projections and packs must evolve their
  `name`/`id` handling together; a divergence would reintroduce the asymmetry this
  RFC removes.
- **Fan-out point.** Four call sites move from the raw config field to the effective
  helper; future source-list readers must use the helper. A documented invariant.
- **Schema/versioning.** New profile shapes, the pack manifest, and the projection
  `name` fields go into `omni-oss/json-schemas`; the round-trip tests
  (`test_source_config_schema_shape_is_unified`) are the guardrail. No new lockfile
  or version obligation (pins by `rev`).

### 6.3 Testability

- **Profile (unit).** The null (`()`)/pack/projection profiles parse; multi-flatten
  unknown-field rejection holds (the `Input<E>` `unit_profile_rejects_unknown_field`
  analogue for `SourceConfig`); `declared_id` returns the per-variant id for
  git/local and `None` for generators/tools; the `Registry` variant is absent from
  serialized output and the schema.
- **Identity (unit).** Projection/pack manifest parses with and without `name`; a
  pre-existing `{routes:[...]}` projection manifest still parses (additive);
  `SourceNameValidator` accepts `@org/name` and rejects malformed names; ledger
  key stability, meaning adding or removing `name` does not change the qualified id
  or re-key links.
- **Recursion (unit).** Reuse the `omni_meta` fake-expander for pack-of-packs: a
  diamond, a cycle (named chain), the depth cap, duplicate sibling ids, `select`
  pruning, all without network.
- **Packs (unit + e2e).** Missing-manifest hard error; both-and manifest;
  all-empty/no-name rejection; `provides` narrowing + **sub-pack cascade**;
  generator/tool item-name namespacing (no collision with a workspace generator of
  the same bare name); the effective-source helpers fan a pack into all three lists
  and dedup against an explicit same-URI entry. E2e: a local both-and pack whose
  manifest declares a generator + tool and composes a sub-pack that projects;
  assert `omni gen` sees the namespaced generator, the sub-pack's `projection sync`
  lands files, `omni pack tree` renders the graph, and `install` materializes each
  commit once.
- **Hard to test.** Concurrent fan-out over the same checkout, covered indirectly
  by the RFC 0013 store's concurrency tests (packs add no new fetch path).

### 6.4 Integration points

- `omni_configurations`: `SourceConfigProfile` (with `impl SourceConfigProfile for ()` as the null profile), the generalized
  `SourceConfig`/`LocalSource`/`GitSource` + stubbed `RegistrySource`; `PackProfile`,
  `PackManifest`, `WorkspaceConfiguration.packs` + `validate_pack_sources`;
  `OwnedProjectionConfiguration` gains `name`/`version`/`description`; validators
  read `declared_id`.
- `omni_projection_configurations`: `ProjectionExtra` split across `BaseExtra`
  (`routes`) and the per-variant `id`.
- `omni_meta`: `member_id` becomes `declared_id`; traversal otherwise unchanged; a new
  `PackMetaExpand` impl lives with the pack contributor. Behaviour-preserving for
  projections (still leaf-only/meta-only, still keyed by the consumer `id`).
- `omni_context` / `omni_api`: the three `effective_*_sources` helpers backed by one
  pack expansion; the four read sites repointed.
- `omni_remote_source_contributors`: `PackMetaExpand`/`PackLeaf`; contributors from
  effective lists. No change to the `RemoteSourceContributor` trait.

Ordering: M1 (profile + migration) lands and is green before M2; M2 (identity + stub)
before M3 (packs). Pack expansion runs after config load and before
discovery/contributor assembly, deterministically (pre-order; pack contributions
appended after explicit entries).

### 6.5 Potential regressions

- **Multi-flatten `deny_unknown_fields` silently breaking.** The whole strict-schema
  guarantee for sources now depends on multiple flatten fields. Mitigated by the
  `Input<E>` precedent (§4.2) and by porting its unknown-field-rejection test to
  `SourceConfig`. If a future serde bump broke it, the ported test fails loudly
  rather than silently accepting ambient fields.
- **Ambient-config leak via the pack manifest.** A pack setting `env`/`capabilities`/
  `enable_experimental` would rewrite the consumer's environment or capability floor.
  Impossible by construction: `PackManifest` omits them and `RawManifest`'s
  `deny_unknown_fields` rejects them (§5.6).
- **Silent projection into the consumer tree.** Mitigated by the author-opt-in gate
  and the cascading consumer `provides` (§5.9): a pack added "for its generators"
  with `provides: [generators]` cannot project, transitively either.
- **Ledger churn from the new `name`.** Explicitly avoided: the ledger keeps keying
  on the consumer `id`/qualified id; `name` is never a key (§5.4). Adding it re-keys
  nothing. (This is why projection naming lands _with_ the profile rather than as a
  separate identity churn.)
- **Pack-id churn orphaning projection links.** A pack's `id` composes the
  `parent::child` qualified id that keys its contributed projections. Renaming the
  `id` re-keys them. Mitigated by treating the declaration-site `id` as the stable key
  (documented) and by reconcile-on-sync (RFC 0004) cleaning orphans. The author
  `name` is deliberately _not_ this key, so an author rename does not churn.
- **Generator/tool name collisions.** Resolved structurally by qualified-id
  namespacing (§5.9) rather than left to a hard error a consumer cannot fix.
- **Path escape / trust inversion / recursion hazards.** Escaping `path`s/targets
  rejected (§5.6); the RFC 0010 floor clamps deeper packs' projection targets;
  `omni_meta` cycle detection + depth cap bound recursion; `omni pack tree` shows
  every transitive author before install.
- **Determinism / caching.** Pack-derived sources pin identically to hand-declared
  ones (same `{uri, rev}` gives the same commit); the pre-order walk is deterministic;
  generator/projection hashing is unchanged.
- **Cross-platform paths.** Local sources reuse existing `path`/glob handling and the
  `parent_root` rebasing meta projections already use; no new path-joining semantics.

### 6.6 Pros and cons

| Option                                                                      | Pros                                                                                                                                                          | Cons                                                                                                                                                     | Verdict |
| --------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- | ------- |
| **A. Profile + two-identifier model + `omni_meta` recursion** (recommended) | Type-level id-optionality; registry a pure addition; one identity model for both source-named subsystems; zero ledger churn; reuses store/manifests/traversal | Foundational-type reach up front; a stub variant no code exercises; two models kept in lockstep                                                          | ✅      |
| B. Runtime validator on a shared `id: Option<String>` (no profile)          | Smallest code                                                                                                                                                 | Invariant is runtime, not type-level; a later registry forces the profile migration anyway; muddies the extra for the two kinds that always require `id` | ❌      |
| C. Make the manifest `name` the identity/ledger key directly                | One identifier, most "author-owned"                                                                                                                           | Not knowable pre-fetch on git/local; re-keys the ledger on rename (the severe churn); breaks pre-fetch selection                                         | ❌      |
| D. Config desugaring of `packs` at load                                     | Simplest mental model                                                                                                                                         | Cannot know a pack's subsystems/sub-packs without fetching, forcing network at config load or double-declaration; loses recursion                        | ❌      |
| E. Do nothing (list it three times; no author name)                         | No code                                                                                                                                                       | Verbose, rev-drift-prone, leaks author layout; source-named units still cannot name themselves                                                           | ❌      |

### 6.7 Prior art and how omni improves on it

| Tool                                                                   | Their approach                                                                                    | What to adopt                                                                | What to change for omni                                                                                                                    |
| ---------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| **Cargo**                                                              | Registry-unique canonical name + optional local `package =` rename to break collisions            | Canonical name is identity; local handle is an optional alias                | omni's git/local world has no registry to enforce the name, so the handle is _required_ there and defaults to name only under the registry |
| **Bazel bzlmod**                                                       | Apparent name (locally chosen, unique among direct deps) vs canonical name (globally unique)      | The two-layer split, and path/qualified-id composition for global uniqueness | omni already composes globally-unique `parent::child` qualified ids in `omni_meta`; reuse them for generator/tool namespacing              |
| **Go modules**                                                         | Import path _is_ the identity, globally unique by construction                                    | Fully-qualified identity needs no registry                                   | omni applies this only to _contributed_ (pack) item names via the qualified id, not to top-level source handles                            |
| **npm**                                                                | Scoped registry-unique name + install aliasing                                                    | Scoped `@org/name` shape (already the `SOURCE_NAME_REGEX`)                   | the name is enforced only by a future registry; pre-registry it is advisory metadata                                                       |
| **Nix flakes**                                                         | One `flake.nix` exposes typed multi-kind outputs; `flake.lock` pins; inputs compose via `follows` | One source, several typed subsystems, one pin, shared lock, composition      | they map onto existing subsystems (no new eval model); the RFC 0013 store _is_ the lock; composition is the `omni_meta` walk               |
| **Nx plugins / VS Code Extension Packs / Backstage / ESLint-Renovate** | One package contributes several kinds; a pack lists other units; author owns defaults             | One install unit fans out to many; author owns projection routes             | git/local (not a package registry) source; workspace-writing projections gated author-opt-in + consumer-cascading `provides`               |

The synthesis adopts the Cargo/Bazel two-layer identity (canonical name over an
immutable pin, with a local handle) and the flake "one pinned source, several typed
subsystems" model, but maps them onto omni's existing subsystems, reuses the `omni_meta`
qualified id as the global-uniqueness mechanism instead of a registry, and keeps the
push/pull asymmetry (auto for pull-only generators/tools, author-opt-in +
consumer-cascading for workspace-writing projections) that none of the prior art
needs.

## 7. Open questions

- **Registry design (deferred; stub contract fixed).** This RFC lands the
  profile, both identity models, and an inert `Registry` stub, but not
  name-to-pin resolution, registry fetch, or registry GC. To keep the identity
  claims in §5.2/§5.4 grounded rather than promissory, the stub carries a fixed
  contract a future registry RFC must preserve:
    - **The pre-fetch key on a registry source is the resolved pin, never the
      `name`.** `declared_id` on a registry source resolves to the pinned commit
      (or its optional-defaulting-to-`name` handle _after_ resolution), so the
      `omni_meta` synchronous pre-fetch constraint that forces a required `id` on
      git/local (I1) is upheld identically; the registry just fills the handle by
      resolution instead of requiring the author to type it.
    - **`name` uniqueness is registry-scoped, never workspace-scoped.** The
      `SourceNameValidator` flips from advisory to enforced only _within a
      registry namespace_; it never becomes a workspace-global uniqueness key, so
      two unrelated registries (or a registry and a local pack) may share a
      `name` without conflict.
    - **`id` stays the ledger / qualified-id key even when it defaults to
      `name`.** On the registry variant `id` becomes optional and defaults to the
      resolved `name`, but it remains the key `omni_meta` composes and the
      projection ledger records, so adopting a registry is churn-free for the
      same reason projection naming is (I4).
      Name-to-pin resolution, fetch transport, and registry GC remain out of scope.
- **Unqualified generator/tool invocation (deferred).** Pack-contributed item names
  are qualified (`agent-kit::house-style:component`, §5.9). An Nx-style shorthand
  that lets a consumer invoke the unqualified name when it is unambiguous would
  soften the ergonomics without weakening the collision guarantee. Not in v1. The
  qualified id from #9 is the guaranteed-correct, cache/ledger-keying handle, and
  any future shorthand is purely a front-end lookup over it (unqualified resolves
  iff unique, else a hard "ambiguous name" error, never a silent pick-one), so it
  can land later without a schema or hashing change. Gated on real usage feedback
  about how deep pack trees get in practice.
- **Consumer-side projection rerouting (deferred).** v1 `provides` is a coarse,
  cascading subsystem gate; the author owns projection routes. Whether a consumer may
  _reroute_/_rename_ a pack's projections without forking is left open; `provides`
  is an enum list (not a bool) so a routing override can be added without a schema
  break. It touches the RFC 0010 trust floor (it inverts who is authoritative
  over where writes land) and the ledger's qualified-id keying (a reroute must
  preserve the key while changing the destination, or add a consumer-side overlay
  the ledger understands, a stale-link risk). Best co-designed with the both-and
  projection question below, since both belong to a future projection RFC.
- **Both-and for projection manifests (deferred).** With `omni_meta::Node`
  generalized, `projection.omni.*` _could_ drop its `Leaf` XOR `Meta` constraint (a
  manifest that both routes and composes). This RFC does not change projection
  route/meta behaviour; only its metadata (`name`). The type change is one dropped
  check, but the semantic change is not: unlike packs (which never write; their
  both-and arms are inert until a subsystem consumes them, and generator/tool item
  collisions are impossible by construction, #9), projections write to destination
  paths, so a node that both routes files and composes route-bearing children
  introduces a new parent-route-vs-child-route conflict axis that XOR structurally
  prevents, plus an undefined ledger identity for the node's own routes. That is the
  design work, and it is identical whether done now or later, so it is left to a
  future projection RFC, co-designed with consumer rerouting above (both are the same
  route model). Deferring spends no option value, since XOR-to-both-and is always
  available later while both-and-to-XOR never is.
- **Per-pack recommended capability floor (deferred, but anticipated).** Whether a
  pack may _declare_ a capability floor its generators/tools recommend and the
  consumer opts into. Capabilities are workspace/subsystem-tagged today; a
  pack-scoped floor is a new scope in that subsystem, and it runs up against
  the #13 no-ambient-fields boundary (a recommended floor has the pack influencing
  the consumer's capability set), so it needs a careful
  recommend-and-explicit-opt-in handshake where "recommended" can never become
  "silently granted." Out of scope for v1, but the implementation should expect it
  to land. Because `PackManifest` deserializes through a private
  `deny_unknown_fields` `RawManifest` + `TryFrom`, a future recommended-capability
  field is a purely additive change to that one struct; the extension point is the
  `RawManifest`, and the #13 boundary is enforced there, so the floor arrives as a
  new _known_ field (an explicit, consumer-gated exception) rather than by loosening
  `deny_unknown_fields`. No v1 type should assume the manifest's field set is
  permanently closed to capability metadata.

## 8. Decision log

| #   | Decision                                                                                                                                                                                                                                                                                                                     | Rationale                                                                                                                                                                                                                                                                                                                  |
| --- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A1  | Generalize `SourceConfig<E>` into a `SourceConfigProfile` (shared `BaseExtra` + per-variant `LocalExtra`/`GitExtra`/`RegistryExtra`) **now**, not later                                                                                                                                                                      | An identifier whose required-ness differs by source kind (required git/local, optional registry) cannot live in a single shared extra; the type-level model makes invalid states unrepresentable and readies the registry with no future migration. Chosen over a runtime validator (option B) for clarity and future ease |
| A2  | Mirror `InputProfile` exactly (marker trait, associated types with the same serde/schema/clone/default/send/sync bounds, multi-flatten variant structs, the paired serde and schemars `bound(deserialize="",serialize="")`); the null profile is `()`, adopted verbatim from `impl InputProfile for ()`, replacing `NoExtra` | It is a shipped, tested precedent; the `unit_profile_rejects_unknown_field` test proves `deny_unknown_fields` survives multi-flatten at omni's pinned serde version, so the pattern is safe to reuse                                                                                                                       |
| A3  | `BaseExtra` is the shared, may-be-empty slot applied to every variant; `id` is the only per-variant field                                                                                                                                                                                                                    | `provides`/`routes` are identical across source kinds, so they belong to the shared slot; `id` is the sole field whose shape depends on the variant, so it is the sole per-variant field. Generators/tools use `()` everywhere                                                                                             |
| A4  | Add the `Registry` variant + `RegistryExtra` now but keep them `#[serde(skip)]`/`#[schemars(skip)]` with a doc comment                                                                                                                                                                                                       | Makes the future registry a pure addition (the per-variant `Option<id>` and `name`/`version` struct fields already exist) without shipping half a registry or exposing an unusable variant on the wire/schema                                                                                                              |
| A5  | Replace `member_id` with `declared_id` (uniform accessor over per-variant extras); read identity via `declared_id`, cross-variant fields via `base()`                                                                                                                                                                        | With per-variant extras there is no single `extra()` type; the traversal still needs a synchronous pre-fetch id, which `declared_id` supplies for git/local                                                                                                                                                                |
| I1  | Two-identifier model for source-named units: required declaration-site `id` (pre-fetch key / ledger key on git/local) + author-declared canonical `name` in the manifest                                                                                                                                                     | The Cargo/Bazel two-layer shape. `member_id` is pre-fetch and synchronous, so the manifest `name` cannot be the pre-fetch key on git/local; the `id` must be. The `name` is identity/coordinate/display, read post-fetch                                                                                                   |
| I2  | The manifest `name` is scoped (`@org/name`) and validated by the pre-existing `SourceNameValidator`; it is **not** a qualified-id segment or a ledger key                                                                                                                                                                    | Registry-ready shape (the validator already exists, unused); keeping it off the ledger key is what makes adding it churn-free                                                                                                                                                                                              |
| I3  | Apply the identity model to **projections too**, in this RFC (RFC 0015 folded in and deleted)                                                                                                                                                                                                                                | Projections and packs are the same structural class (source-named units); the profile is subsystem-agnostic, so the template _is_ the profile, and both consume it. Splitting projection naming into its own RFC would overload the dependency and risk two divergent models                                               |
| I4  | Projection naming is **churn-free**: `OwnedProjectionConfiguration` gains optional `name`/`version`/`description` (tag-free, additive, via the existing `RawManifest`+`TryFrom`), and the ledger keeps keying on the consumer `id`/qualified id                                                                              | The severe risk (RFC 0015's) only exists if `name` becomes the ledger key; keeping the consumer `id` as the key structurally avoids it, so naming lands safely with the profile rather than as a later migration                                                                                                           |
| I5  | Registry stays deferred, but the stub carries a fixed contract (pre-fetch key is the resolved pin not the `name`; `name` uniqueness is registry-scoped never workspace-global; `id` remains the ledger/qualified-id key even when defaulting to `name`)                                                                      | Grounds the identity claims in §5.2/§5.4 in invariants a future registry RFC must preserve, closing the "asserts behaviour it hasn't designed" gap without expanding v1 scope                                                                                                                                              |
| 1   | Name the concept `packs`                                                                                                                                                                                                                                                                                                     | Short, evokes "extension pack"; avoids omni's overloaded `extension`/`plugin` terms                                                                                                                                                                                                                                        |
| 2   | Model packs as an effective-source layer feeding the three subsystem lists, not a new subsystem                                                                                                                                                                                                                              | Reuses the RFC 0013 store (dedup) and per-subsystem discovery; avoids duplicating three discovery paths                                                                                                                                                                                                                    |
| 3   | Expansion at a use-time point, not a config-load desugaring                                                                                                                                                                                                                                                                  | Keeps config load offline/pure and leaves the parse-time duplicate-URI validators intact                                                                                                                                                                                                                                   |
| 4   | Pack-of-packs in v1 via `omni_meta::expand`                                                                                                                                                                                                                                                                                  | Reuses cycle detection, depth cap, qualified ids, and selection instead of a second recursion engine                                                                                                                                                                                                                       |
| 5   | `pack.omni.*` is its own type (subsystem-source slice + identity/metadata), not `WorkspaceConfiguration`                                                                                                                                                                                                                     | Avoids the schema conflict (`projects` required, ambient fields) of reusing the workspace type; "focused workspace config" is the intuition, not type reuse                                                                                                                                                                |
| 6   | Manifest required, root-only, no auto-detection                                                                                                                                                                                                                                                                              | Keeps discovery unambiguous; gives every pack a stable identity; manifest-less sources still work via the existing lists                                                                                                                                                                                                   |
| 7   | A pack is a focused workspace: may declare its own sources AND compose sub-`packs` in one node (both-and `omni_meta::Node`, already landed)                                                                                                                                                                                  | Nothing semantic forces the choice (the workspace does both); the engine change is behaviour-preserving; projections keep their XOR by never emitting a both-and node                                                                                                                                                      |
| 8   | A pack contributes projections iff its manifest declares `projections:`; consumer `provides` narrows and **cascades** to sub-packs                                                                                                                                                                                           | Manifest-required makes the opt-in trivial; cascading `provides` is the safe reading for the projection-write concern (a narrowed pack cannot transitively project)                                                                                                                                                        |
| 9   | Pack-contributed generator/tool **item** names are namespaced by the pack's `omni_meta` qualified id                                                                                                                                                                                                                         | Item names are workspace-globally-unique (dup = hard error); qualifying by the composed id makes them collision-free by construction (Bazel/Go), consistent with the projection contribution, and removes precedence questions                                                                                             |
| 10  | No new lockfile, ref namespace, or GC path; refs recorded under each consuming subsystem                                                                                                                                                                                                                                     | The shared store already dedups by commit and union-GCs                                                                                                                                                                                                                                                                    |
| 11  | `omni pack` is read-only (`list`/`tree`/`info`); installation stays `remote-sources install`                                                                                                                                                                                                                                 | Packs are just sources; the existing install path already materializes them                                                                                                                                                                                                                                                |
| 12  | Manifest `version` is metadata only, not a constraint, not a diamond key                                                                                                                                                                                                                                                     | omni pins by `rev` and does no version solving; a constraint-shaped field would mislead                                                                                                                                                                                                                                    |
| 13  | `PackManifest` carries no ambient fields; `deny_unknown_fields` on the private `RawManifest` makes them a parse error                                                                                                                                                                                                        | Prevents a pack from silently mutating the consumer's environment, capability floor, sandbox toggle, or ignore block, a security boundary by construction, not convention                                                                                                                                                  |
| 14  | Pack paths are pack-root-relative; `..`/absolute escapes are rejected                                                                                                                                                                                                                                                        | Keeps a pack self-contained and preserves RFC 0013's content-addressed, disposable-checkout invariant                                                                                                                                                                                                                      |
| 15  | Sub-pack list is named `packs`, not `sources`                                                                                                                                                                                                                                                                                | A pack's own leaf arm is itself three source lists, so `sources` would be ambiguous; `packs` matches the top-level `packs:` field                                                                                                                                                                                          |
| 16  | Title expanded to "Source packs, identity, and the source-config profile"                                                                                                                                                                                                                                                    | The RFC now carries a foundational type change and both source-named identity models beyond the packs themselves; the title should reflect that scope while keeping packs the motivating story                                                                                                                             |
