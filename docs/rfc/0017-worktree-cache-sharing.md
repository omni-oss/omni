# RFC 0017: Automatic worktree cache sharing

- **Status:** Accepted — implemented (pending merge)
- **Owner:** TBD
- **Date:** 2026-10-29
- **Base branch:** `feat/custom-cache-dir`
- **Affected crates:** `omni_scm`, `omni_context`, `omni_configurations`, `omni_constants`, `omni_ignore_contributors`, configuration discovery and collection, `omni_api`, `omni_cli_core`, integration tests

## 1. Summary

Automatically reuse the primary checkout's cache from related VCS worktrees.
Keep checkout discovery VCS-neutral and implement repository-specific knowledge
inside SCM providers, not cache code.

Add an ignored, machine-local `worktree.omni.{ext}` configuration at the Omni
workspace root. The primary workspace supplies inherited cache configuration;
each linked workspace may override it. Relative paths resolve against the
workspace containing the file that supplied the value, never the invocation
directory or the inheriting worktree.

Discover checkout relationships once per context construction. Keep them during
ordinary configuration reloads. Do not rediscover repositories or reread this
configuration for every task, project, input file, or cache access.

## 2. Goals and non-goals

### Goals

- Reuse existing primary-workspace caches without extra configuration.
- Let users choose a shared cache location once in the primary workspace.
- Allow intentional checkout-local overrides.
- Preserve all existing explicit cache-directory overrides and their semantics.
- Keep workspace roots and execution isolated while sharing cached results.
- Provide predictable behavior for nested workspaces and long-lived consumers.
- Integrate machine-local configuration with `omni ignore sync`.

### Non-goals

- Sharing caches between independent clones based on their remote or commits.
- Synchronizing or copying configuration files between worktrees.
- Migrating or deleting existing checkout-local caches.
- Changing task cache keys, artifact formats, or remote-cache protocols.
- Expanding shell expressions in configuration paths.
- Electing a primary checkout when the VCS does not identify one reliably.

## 3. Configuration

The file lives beside `workspace.omni.{ext}`, not at an arbitrary repository root:

```yaml
# worktree.omni.yaml — machine-local; do not commit
cache:
  dir: ../shared-cache/app
```

Recognize the existing supported configuration extensions: `yml`, `yaml`, `json`,
and `toml`. More than one worktree configuration variant in the same workspace
is an error, rather than an extension-priority choice.

The initial schema contains only optional `cache.dir`. An empty configuration or
an omitted or `null` field means inheritance, consistent with other Omni
configuration. An explicitly empty string is an error. Reject unknown fields to
catch misspellings. No environment-specific file variants, `extends`, or general
manifest merging are introduced.

### 3.1 Precedence

Use the first applicable cache-directory source:

1. Existing explicit override: CLI/environment or API/configuration input, with
   precedence within those interfaces unchanged.
2. Current workspace's `worktree.omni.*` `cache.dir`.
3. Corresponding primary workspace's `worktree.omni.*` `cache.dir`.
4. Corresponding primary workspace's `.omni/cache`.
5. Current workspace's `.omni/cache` if no usable primary workspace is known.

For the primary workspace, local and inherited configuration are the same file
and are loaded only once. A linked workspace with no `cache.dir` inherits the
primary value, even if it has a worktree configuration file of its own.

An explicit override bypasses worktree discovery and worktree configuration
loading entirely, including errors in otherwise unused worktree files.

Without an explicit override, validate the current configuration first. If it
supplies `cache.dir`, do not load the shadowed primary configuration. Otherwise,
load and validate the primary configuration when available. Errors in selected
configuration must not silently become a different cache location.

### 3.2 Path semantics

- Absolute paths are allowed.
- Relative paths, including `..`, resolve against the workspace containing the
  configuration file that supplied the value.
- Existing CLI/API relative-path semantics remain unchanged: invocation-relative.
- Empty or whitespace-only paths are errors; do not trim meaningful path bytes.
- `~` home expansion and environment-variable interpolation are unsupported in
  this release. Recognizable expansion forms such as `~/cache`, `$HOME/cache`,
  `${HOME}/cache`, and `%LOCALAPPDATA%/cache` receive an actionable error rather
  than becoming unintended literal paths. Ordinary literal filenames containing
  these characters are not categorically forbidden.
- Honor platform-native absolute paths, including Windows drive and UNC paths;
  do not treat Windows drive-relative forms such as `C:cache` as absolute.
- Reuse existing-prefix canonicalization from the custom-cache-dir branch. A
  cache directory need not exist yet. Preserve filesystem semantics for symlinks
  and `..`; do not blindly collapse them lexically.

### 3.3 Unsafe cache locations

Reject an effective cache directory that equals or is an ancestor of either the
current Omni workspace or its known corresponding primary Omni workspace. Cache
exclusion could otherwise hide source inputs, and cache maintenance could operate
on source directories. Apply the guard after path resolution, accounting for
filesystem aliases and symlinks; do not rely on string-prefix comparisons.

For primary `/repos/app` and linked workspace `/worktrees/app-feature`, reject
`/repos/app`, `/repos`, `/worktrees/app-feature`, `/worktrees`, and `/`, including
symlink aliases resolving to these locations. Allow dedicated descendants such
as `/repos/app/.omni/cache`, sibling caches such as `/repos/shared-cache/app`,
and external dedicated cache directories.

Apply this safety guard to every selected source, including explicit overrides.
An explicit override still skips VCS discovery, so validate against the current
workspace and any already-known primary workspace; do not claim to protect an
undiscovered primary checkout. This is an intentional safety restriction on
previously accepted override values, not a change to override precedence or
relative-path origins.

The guard cannot identify unrelated valuable directories elsewhere on disk.
Document that users must select a dedicated cache directory, not a directory
containing other files they want to preserve.

For `/repos/app/worktree.omni.yaml` containing `../shared-cache/app`, both the
primary workspace and inheriting linked workspaces use `/repos/shared-cache/app`.

If `/worktrees/app-feature/worktree.omni.yaml` contains `.omni/cache`, that
workspace instead uses `/worktrees/app-feature/.omni/cache` intentionally.

## 4. VCS-neutral checkout discovery

Add a dedicated discovery API to `omni_scm`, independent of affected-file
selection and `SelectScm::None`. Suggested shape, not a fixed public API:

```rust
pub struct CheckoutLayout {
    pub checkout_root: PathBuf,
    pub primary_checkout_root: Option<PathBuf>,
}

pub fn discover_checkout(
    path: &Path,
) -> Result<Option<CheckoutLayout>, DiscoveryError>;
```

`None` means no supported repository was found. A missing primary root means a
repository was found but no reliable primary checkout is available. Retain a
diagnostic reason internally so unsupported, absent, and broken metadata can be
distinguished.

Providers must use authoritative local repository metadata. Never infer a
relationship from matching remotes, branches, commits, or directory names.
Do not select an arbitrary worktree from a list as the primary.

### 4.1 Provider scope

The first implementation adds the neutral API and a Git provider using the
existing `gix` dependency. Support linked worktrees, detached worktrees, `.git`
files, and separate metadata directories without assuming that the parent of
the common Git directory is a checkout. Submodules are independent repositories,
not worktrees of the enclosing repository. A bare repository without an
authoritative primary checkout uses the local fallback.

Git is the initial provider, not the cache policy. Do not advertise universal
VCS support. Jujutsu/native workspace support and other VCS providers are
follow-ups behind the same contract. A future Jujutsu provider must take
precedence over Git discovery in colocated repositories and must not assume that
every Jujutsu workspace family has a usable primary workspace.

Discovery uses local metadata, without network calls or a per-task VCS process.
Tests may invoke VCS CLIs to construct real repository fixtures.

### 4.2 Nested Omni workspaces

Compute the current workspace's path relative to its checkout root. Apply that
same relative path to the primary checkout root to locate the corresponding
primary Omni workspace.

For example, `/worktrees/app/tools/docs` maps to `/repos/app/tools/docs`, not
`/repos/app`. Require the mapped directory to exist and contain a supported
workspace marker. Do not load its project graph or require equal configuration
contents: branches may differ and task keys remain responsible for validity.

Reject mappings that escape the primary checkout through symlinks or cannot be
established safely. If the corresponding workspace is absent, do not borrow an
unrelated parent workspace's configuration or cache; use the local fallback.

## 5. Resolution and failure behavior

Resolve a cache directory before loading remote-cache configuration. Retain the
resolved absolute path, its source, the declaring file when applicable, and the
checkout relationship needed for reloads.

- No repository or unsupported VCS: local configuration/default; debug diagnostic.
- Broken discovery metadata, unavailable primary checkout, or missing mapped
  workspace: local configuration/default; diagnostic explaining lost sharing.
- Malformed selected configuration, duplicate variants, or invalid selected
  path: error with the file and field identified.
- Selected cache is inaccessible or unwritable: normal cache error, not silent
  fallback to another location.
- A configured directory that does not exist may be created by existing cache
  operations; resolution itself must not create it just to canonicalize it.

Keep `omni cache dir` printing only the effective path. Debug logging explains
whether it came from an explicit override, local file, primary file, automatic
primary default, or local fallback. Do not log credentials.

## 6. Context lifetime and performance

### Initial construction

Perform discovery once per constructed context when no explicit override is
present. Read only the small configuration files needed for resolution. A normal
CLI invocation constructs one context, so this is normally a one-time startup
cost, not a process-global guarantee.

Do not add a process-global path cache: one API/MCP process may manage multiple
workspaces, and repository layouts can change.

### Ordinary reload

Retain the discovered relationship. Include candidate local and primary
worktree-configuration paths, including absent variants, in the configuration
fingerprint so creation, removal, edits, and duplicate variants trigger reload.
Use the existing fingerprint mechanism; no per-cache-access filesystem checks.

Re-resolve file-derived directories after a configuration change. Do not pass a
previously resolved directory back as an explicit override: the current
`Context::reload` pattern would otherwise freeze the old selection. Preserve
actual explicit overrides separately from resolved state.

After a directory change, load remote-cache configuration and compute subsequent
fingerprints against the new cache. Retain existing active-context snapshot
semantics so an in-flight operation does not mix old and new cache locations.

VCS topology changes require explicit context reconstruction/refresh; ordinary
configuration reload does not rediscover worktrees. The next CLI invocation
always constructs a fresh context. Do not add a background topology watcher.

## 7. Sharing boundaries and compatibility

Only the effective cache location changes. Keep source discovery, workspace and
project configuration, task execution, output destinations, scratch state, and
traces rooted in the current workspace.

Reuse guarantees already supplied by `feat/custom-cache-dir`:

- Content-addressed task results and existing publication/pruning locks.
- Per-checkout file-hash indexes, even when cache storage is shared.
- Cache-directory exclusions from collection and discovery.
- Cross-filesystem copy fallback where hardlinks are unavailable.
- Cache-scoped remote-cache configuration and encryption-key identity.

Consequently, inheriting a shared cache also inherits its remote-cache
configuration and credentials. Cache clearing, pruning, and inspection affect
the shared store. Document these consequences and treat sharing as a local trust
decision among related checkouts, not an isolation mechanism for untrusted code.

Do not change task keys merely because a different cache source was selected.
Identical cacheable inputs may hit across checkouts; different inputs must miss.
Existing hardlink restoration and mutation-safety guarantees must remain intact.

No cache migration, symlink installation, automatic configuration writes, or
modification of another checkout occurs during resolution.

## 8. Ignore, discovery, and hashing integration

Define `WORKTREE_OMNI = "worktree.omni.{ext}"` in `omni_constants` and reuse it
in the loader, fingerprinting, tests, and ignore contributor.

Have `InternalContributor` emit this workspace-root-anchored managed pattern:

```gitignore
/worktree.omni.*
```

`omni ignore sync` maintains it in every configured ignore target, not just
`.gitignore`. Preserve the existing managed-block and idempotence behavior.

Worktree configuration is local control-plane state. Exclude supported worktree
configuration filenames from ordinary manifest discovery and default file-hash
collection even before ignore synchronization. Load them directly, regardless
of ignore rules. Keep them out of generic workspace manifest hashing so a
machine-local cache-path edit does not invalidate task results. This exclusion
does not hide changes to actual task inputs or override explicitly declared
input behavior.

Do not ignore all of `.omni`, unrelated manifests, or arbitrary external cache
parents. Continue excluding the effective cache directory through existing
cache-directory handling. If projection protection enumerates control-plane
manifests, protect worktree configuration from accidental projection writes too.

## 9. High-level implementation hints

1. **Configuration and constants:** add the small typed worktree configuration
   schema and shared filename constant. Keep path validation and declaring-file
   origin explicit; do not reuse invocation-relative override resolution.
2. **SCM discovery:** add the provider-neutral layout contract and Git adapter.
   Keep it separate from diff/affected APIs. Cover real worktrees with filesystem
   fixtures rather than manually approximating Git metadata.
3. **Context resolver:** extract cache selection from `Context::new_with_cache_dir`
   into a small resolver accepting explicit input, workspace root, and a
   discoverer. Return resolved path plus provenance and reload state. Inject
   discovery in unit tests; avoid forcing real VCS access into mocked contexts.
   Validate the selected cache against known workspace roots before applying
   cache exclusions or loading cache-owned configuration.
4. **Reload:** store original override input separately, retain topology across
   ordinary reloads, and extend fingerprints with configuration candidates. Keep
   remote-cache loading downstream of resolution.
5. **Collection and ignore:** add the managed ignore pattern and built-in
   control-plane exclusions through existing shared filtering mechanisms. Do
   not scatter filename string checks through hashers.
6. **Consumers:** keep CLI, API, and MCP using the context's resolved cache path.
   Preserve output formats and explicit override precedence; no new CLI flag is
   required. Review constructors/configuration fingerprints in long-lived users.
7. **Documentation:** document provider scope, examples, local overrides,
   invocation-relative versus file-relative paths, shared credential/pruning
   effects, and the topology refresh boundary.

## 10. Acceptance tests

### Resolution and configuration

- Primary checkout and linked worktrees select the same automatic cache.
- Primary relative configuration resolves against the primary workspace from
  every inheriting checkout, regardless of invocation directory.
- Local worktree values override inherited values; absent fields inherit.
- Absolute paths, `..`, Windows drive/UNC paths, filesystem aliases, and
  nonexistent cache suffixes behave correctly.
- Null values inherit, including `cache: null` and `cache.dir: null`.
- Empty paths, unsupported expansion syntax, malformed files, unknown
  fields, and duplicate variants produce clear errors.
- Explicit overrides retain existing precedence and relative semantics, skip
  discovery, and bypass otherwise invalid worktree configuration.
- Cache paths equal to or containing a known current/primary workspace are
  rejected for every selection source, including filesystem aliases and
  symlinks. Dedicated descendants, siblings, and external directories remain
  valid; explicit overrides do not trigger discovery solely for this guard.

### Discovery and workspace boundaries

- Ordinary/detached linked worktrees, `.git` files, separate metadata directories,
  nested workspaces, and supported submodule layouts.
- Independent clones never share automatically.
- No VCS, unsupported VCS, bare repositories, missing primary checkouts, missing
  mapped workspaces, broken metadata, and unsafe symlink mappings fall back
  predictably without borrowing an unrelated workspace.

### Correctness and lifecycle

- A result produced in the primary restores into the linked workspace without
  executing the task again; outputs do not land in the primary workspace.
- Changed inputs miss, including different bytes with equal size/mtime across
  checkouts, proving file-hash indexes remain checkout-isolated.
- Concurrent publication, restoration, and pruning preserve existing locking
  guarantees; cross-filesystem restoration still works.
- Shared remote configuration and encryption identity work across checkouts.
- Configuration edits/creation/removal switch file-derived directories on reload;
  explicit overrides stay fixed and remote configuration follows the new path.
- Instrumented discovery runs once per initial context, zero times per task and
  ordinary reload, and again on explicit context reconstruction.
- Ignore synchronization is idempotent for every configured target. Local
  configuration is excluded from default hashes before synchronization, and
  editing only `cache.dir` leaves task hashes unchanged.

## 11. Rollout

Implement on top of `feat/custom-cache-dir` without a cache-format migration.
Land the configuration/resolver and SCM provider as testable units, then wire
ignore handling, reloads, end-to-end coverage, and user documentation before
enabling automatic selection. The first release explicitly supports Git
worktrees through a VCS-neutral interface; additional providers are independent
follow-ups, not special cases in cache resolution.
