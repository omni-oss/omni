# RFC 0011: Standardize glob configuration on a structured include/exclude type

- **Status:** Implemented <!-- Draft | Accepted | Implemented | Superseded -->
- **Owner:** TBD
- **Date:** 2026-09-03
- **Affected crates/files:** new `omni_glob_config` crate (`GlobConfig<T>` and `MergeGlobConfig<T>`, depends on `omni_glob`); `omni_glob` (owns the normalized `GlobPatterns<T>` and the `ToGlobPattern` trait; `GlobMatcher` gains an include/exclude constructor; the leading-`!` scan and `\!` escape are removed); `omni_config_types` (new `MergeSingleOrMany<T>`, which pulls in `config_utils` for `ListConfig`); `omni_configurations` (`cache.key.files`, `cache.output.files`, workspace `projects`); `omni_generator_configurations` (add-many / transform-many `files`); `omni_projection_configurations` (`PatternRule`/`FlattenRule` `match`, `MirrorProjection` `scope`); `omni_collector`, `omni_execution_plan`, `omni_cache` (normalize config into `GlobPatterns`, not from `!`); the JSON schema for every migrated field.
- **Supersedes:** RFC 0009 (negation in `input_files` / `output_files` via a leading `!`).
- **Amends:** RFC 0005 (list-valued `match`/`scope` with `!` negation), RFC 0007 (the shared `GlobMatcher`), RFC 0008 (the `!` parity convention and the `\!` escape). The include/exclude split those RFCs built inside `GlobMatcher` stays; the leading-`!` syntax that drove it is removed.
- **Depends on:** RFC 0007 (`GlobMatcher`), RFC 0008 (`include_set` and the memoized builder in `omni_glob`).

## 1. Summary

Every glob field in omni config is a flat list today, and a flat list of patterns reads like a `.gitignore`: authors expect a leading `!` to exclude, and expect a later entry to re-include what an earlier one dropped. omni does neither. Discovery, generators, and projections split the list into an include set and an exclude set by counting leading `!`, then match unordered (`include && !exclude`). Cache input and output files ignore `!` entirely and treat it as a literal. Same syntax, three different meanings.

This RFC replaces the leading-`!` convention with one structured type, `GlobConfig<T>`, used by every glob config field in the workspace. A field is either a bare pattern, a list of patterns (both include-only), or an object with explicit `include` and `exclude` lists. Exclusion is expressed by structure, never by a `!` prefix. `exclude` always wins, unordered, matching Bazel's `glob(include, exclude)` and Rollup's `createFilter`. The `!` marker and the `\!` escape from RFC 0008 are removed. This is a breaking config change, taken deliberately while omni is pre-release, because it removes a real ambiguity rather than papering over it.

## 2. Motivation / Problem

The trigger was RFC 0009, which set out to teach `cache.key.files` to exclude with a leading `!`. Digging into it exposed the deeper problem: a list of glob strings is the wrong shape for omni's semantics.

- **A list implies gitignore, omni is not gitignore.** gitignore and Biome evaluate a pattern list last-match-wins, so a later `!` re-includes a subset of an earlier exclude. omni's `GlobMatcher` is unordered. It computes the include-set minus the exclude-set, with no re-inclusion. An author who writes `["src/**", "!src/gen/**", "src/gen/keep.rs"]` expecting the last entry to win gets a silent surprise.
- **The same syntax means different things per field.** `!` negates in discovery, generators, and projections (RFC 0005, 0007, 0008), and is a dead literal in `cache.key.files` / `cache.output.files` (RFC 0008 kept those include-only). A contributor has to know which field they are editing to know what `!` does.
- **The `\!` escape is a fix for a problem the syntax created.** RFC 0008 added `\!` so a file literally named `!thing` stays reachable once `!` means exclude. Structured include/exclude has no leading marker, so a literal `!` is just an ordinary pattern character and needs no escape at all.

The build-tool world settled this long ago. Bazel `glob(include=[...], exclude=[...])`, Rollup/Vite `createFilter(include, exclude)`, tsconfig `include`/`exclude`, and webpack loader `include`/`exclude` all use two explicit inputs and unordered exclude-wins. The `!`-in-one-list form belongs to ignore files walked top to bottom, which omni does not do. omni should adopt the build-tool shape it actually implements.

Desired outcome: one glob config type, one meaning for every field, exclusion by structure, and no leading-marker parsing anywhere.

## 3. Goals / Non-goals

### Goals

- Introduce two config types, `GlobConfig<T>` (last-writer-wins) and `MergeGlobConfig<T>` (layered), in a new `omni_glob_config` crate, and use them for every glob config field: cache input/output files, workspace `projects` discovery, generator `files`, and projection `match`/`scope`. Both normalize into `omni_glob::GlobPatterns<T>`.
- Express exclusion only through the `IncludeAndExclude { include, exclude }` variant. `exclude` always wins, unordered. The bare and list forms are include-only.
- Remove the leading-`!` convention and the `\!` escape from every config-driven caller, along with the scan that implements them in `omni_glob`.
- Keep the hashed paths (cache input/output files) byte-identical for any config that is pure-include after migration, proven by the RFC 0008 golden-hash snapshot.
- Keep the collector's `Candidate` fast path and single global walk, and keep `DefaultTaskScmAffectedFilter` reading the same split the collector hashes.

### Non-goals

- Ordered, gitignore-style re-inclusion. Rejected: omni is unordered, and Section 7 of RFC 0009 plus the prior-art survey below settle this.
- Runtime project/task/dir filters and `get_stats` globs (the CLI/MCP selection filters). Those are not config fields; they take patterns from the command line. They can adopt `include`/`exclude` flags later, out of scope here.
- Policy matching over rooted paths (`omni_capabilities::matching`).

## 4. Background: current mechanisms

Two building blocks already exist and this RFC composes them rather than inventing new ones.

- **`SingleOrMany<T>`** (`omni_config_types/src/single_or_many.rs`) is an untagged enum `Single(T) | Many(Vec<T>)` with serde, schemars, and a last-writer-wins `Merge`. Projection `match`/`scope` already use it. It is the include-only half of the proposed type.
- **`ListConfig<T>`** (`config_utils/src/list_config.rs`) is an untagged enum `Value(Vec<T>) | Merge{merge} | Append{append} | Prepend{prepend} | Replace{replace}`, disambiguated by object keys. It is how `cache.key.files` layers a project's entries onto workspace defaults. Its append/prepend semantics are the reason the merge model (Section 7) is not trivial.

The glob config fields today, verified in source:

| Field                                          | Type today                                                 | `!` meaning today                 | Feeds the hash?         |
| ---------------------------------------------- | ---------------------------------------------------------- | --------------------------------- | ----------------------- |
| `cache.key.files`                              | `ListConfig<OmniPath>` (`cache_configuration.rs:80`)       | literal (include-only, RFC 0008)  | yes                     |
| `cache.output.files`                           | `ListConfig<OmniPath>` (`task_output_configuration.rs:22`) | literal (include-only, RFC 0008)  | yes, via output capture |
| workspace `projects` (discovery)               | `Vec<String>` (`workspace_configuration.rs:36`)            | exclude, `!!` parity, `\!` escape | no                      |
| generator `files` (add-many, transform-many)   | `Vec<PathBuf>` (`action_configuration.rs:182`)             | exclude, `!!` parity, `\!` escape | no                      |
| projection `PatternRule`/`FlattenRule` `match` | `SingleOrMany<String>` (`projection.rs:162`, `:180`)       | exclude, `!!` parity, `\!` escape | no                      |
| projection `MirrorProjection` `scope`          | `Option<SingleOrMany<String>>` (`projection.rs:209`)       | exclude, `!!` parity, `\!` escape | no                      |

The negation-aware fields all run through `omni_glob::GlobMatcher`, which strips leading `!` markers, splits include from exclude on bang parity, and matches `include.is_match && !exclude.is_match`. The cache fields run through `omni_glob::include_set`, which compiles a raw `Arc<GlobSet>` and treats `!` as a literal. RFC 0008's `scan_markers` and `\!` escape live inside `GlobMatcher` and back the parity classification.

## 5. Proposed design

### 5.1 The normalized target and the config types

Every glob config normalizes to one owned form, which lives in `omni_glob`:

```rust
// omni_glob
pub struct GlobPatterns<T> {
    pub include: Vec<T>,
    pub exclude: Vec<T>,
}

pub trait ToGlobPattern {
    /// Pattern text for the glob compiler. Path-like types normalize
    /// separators to `/`. Rooted resolution (an `OmniPath` `@workspace/`
    /// prefix) is not done here; it stays with the consumer, which holds the
    /// root map.
    fn to_glob_pattern(&self) -> std::borrow::Cow<'_, str>;
}
```

`GlobPatterns<T>` is what every consumer compiles. The collector resolves each side against the root map and builds two `include_set`s; a `GlobMatcher` is built from the two lists directly. It owns `Vec<T>` rather than pattern strings, so rooted resolution stays a consumer step (decision 7), and `ToGlobPattern` sits beside it because compilation is the boundary that needs the text.

`omni_glob_config` depends on `omni_glob` and holds the two config-facing types. They share the same three forms and differ only in how they merge across config layers. Fields that layer a project onto workspace defaults today (the cache files, on `ListConfig`) use the merging form; fields that do not (projection `match`/`scope`, discovery `projects`, generator `files`) use the plain form.

```rust
// omni_glob_config: last-writer-wins merge, matching SingleOrMany
pub enum GlobConfig<T> {
    Single(T),                                    // scalar
    Many(Vec<T>),                                 // sequence
    IncludeAndExclude {                           // object
        include: SingleOrMany<T>,
        exclude: SingleOrMany<T>,
    },
}

// omni_glob_config: per-side ListConfig layering
pub enum MergeGlobConfig<T> {
    Single(T),
    Many(MergeSingleOrMany<T>),
    IncludeAndExclude {
        include: MergeSingleOrMany<T>,
        exclude: MergeSingleOrMany<T>,
    },
}
```

The layering form needs a value type that carries both the scalar-or-sequence ergonomics and `ListConfig`'s `append`/`prepend`/`replace`/`merge` semantics. `SingleOrMany` has the ergonomics but merges last-writer-wins; `ListConfig` has the merge forms but no bare scalar. So this RFC adds `MergeSingleOrMany<T>` in `omni_config_types`, beside `SingleOrMany`, where it depends on `config_utils` for `ListConfig` (`omni_config_types` sits above `config_utils`, and `config_utils` has no back-edge to it, so there is no cycle):

```rust
// omni_config_types: a bare scalar, or any ListConfig form
pub enum MergeSingleOrMany<T: Merge> {
    Single(T),
    List(ListConfig<T>),
}
```

`MergeSingleOrMany` reuses `ListConfig` wholesale for the list and layering forms and adds only the bare scalar. Its `Merge` normalizes `Single(x)` to a one-element list and delegates to `ListConfig::merge`, so a project's `append` still layers onto a workspace default. `GlobConfig` merges last-writer-wins; `MergeGlobConfig` merges each side through `MergeSingleOrMany`, which is to say through `ListConfig`. Merge runs on the config type first, so layering happens on the config representation, and normalization to `GlobPatterns<T>` is the final step. `Single(x)` and `Many(xs)` normalize to `include`-only with an empty `exclude`, which keeps a pure-include config byte-identical.

All forms deserialize untagged with a hand-written `UntaggedEnumVisitor`, the pattern already used by `CommandConfig`, `ListConfig`, and `DictConfig`. A scalar is `Single`, a sequence is `Many`, and an object with `include`/`exclude` keys is `IncludeAndExclude`. In `MergeGlobConfig`, any other sequence or object is delegated to `ListConfig`, which already parses the list plus the `append`/`prepend`/`replace`/`merge` forms. The shapes are told apart by JSON kind and key set, so they never collide.

`ToGlobPattern` is implemented for `String`, `&str`, `PathBuf`/`Path` (separators normalized to `/`), and `OmniPath` (its display form). It lets the set-builder produce pattern text without caring about `T`; it does not resolve roots, so an `OmniPath` still resolves against the root map in the consumer.

Every non-`IncludeAndExclude` form is include-only. Exclusion is expressible only through the object form, so a flat list can no longer secretly negate.

Appendix (Section 9) has the worked code: the deserializers, the `Merge` impls, `normalize`, and a config-to-`GlobPatterns` walk-through.

### 5.2 Removing the `!` convention

The leading-`!` parity rule and the `\!` escape are removed from `omni_glob`. `scan_markers`, `is_include`, and the escape table go away, and `GlobMatcher::new`/`GlobMatcher::rooted` stop parsing markers. `GlobMatcher` keeps its two-set structure but gains an explicit constructor:

```rust
impl GlobMatcher {
    pub fn from_globs<S: AsRef<str>>(
        include: &[S],
        exclude: &[S],
        opts: GlobOptions,
    ) -> Result<Self, globset::Error>;

    // rooted variant prefixes both lists with the root, as today.
}
```

Callers now hand `GlobMatcher` two explicit lists sourced from a `GlobConfig`. A leading `!` in any pattern is an ordinary character from this point on, matched literally, on every platform, with no escape. This reverts the RFC 0008 escape work for config callers, which is the correct outcome: the escape only existed to survive the `!` convention this RFC deletes.

### 5.3 Site-by-site migration

Every field in the Section 4 table changes type. Cache files layer today (`ListConfig`), so they take `MergeGlobConfig`; the rest do not layer, so they take `GlobConfig`:

| Field                | New type                                               |
| -------------------- | ------------------------------------------------------ |
| `cache.key.files`    | `MergeGlobConfig<OmniPath>`                            |
| `cache.output.files` | `MergeGlobConfig<OmniPath>`                            |
| workspace `projects` | `GlobConfig<String>`                                   |
| generator `files`    | `GlobConfig<PathBuf>`                                  |
| projection `match`   | `GlobConfig<String>` (replaces `SingleOrMany<String>`) |
| projection `scope`   | `Option<GlobConfig<String>>`                           |

Config that was pure-include keeps its bare or list form unchanged, so `files: ["src/**"]` parses as `GlobConfig::Many(["src/**"])` and behaves as before. Config that relied on `!` to exclude must move the excluded patterns into an `exclude` list:

```yaml
# before (discovery, generator, projection)
match: ["src/**", "!src/gen/**"]

# after
match:
    include: ["src/**"]
    exclude: ["src/gen/**"]
```

### 5.4 How each subsystem consumes it

Every consumer first normalizes its config value (after merge) into `omni_glob::GlobPatterns<T>`, then compiles that:

- **Negation-aware callers (discovery, generators, projections)** build a `GlobMatcher` with `from_globs(include, exclude, opts)` from the normalized `GlobPatterns` (an empty `exclude` for the bare/list forms). Matching is unchanged: `include.is_match && !exclude.is_match`.
- **The collector** normalizes `cache.key.files` / `cache.output.files` into `GlobPatterns<OmniPath>`, resolves and absolutizes each side against the root map, then compiles two `include_set`s per file kind: one for `include`, one for `exclude`. The match loop becomes `include.is_match_candidate(c) && !exclude.is_match_candidate(c)`. Only `include` seeds the walk (`includes`, `match_bases`, `forced_includes`, `topmost`); an exclude is a per-project filter and never forces a directory to be walked. This is the RFC 0009 collector change without the `!`-classification step, because the split arrives pre-separated in `GlobPatterns`.
- **`DefaultTaskScmAffectedFilter`** normalizes the same config into the same `GlobPatterns` and builds the same two sets, so a changed file counts as affecting the task when it matches `include` and not `exclude`. The filter and the collector normalize one structure the same way, so the affected-set can never diverge from the hashed-set.

### 5.5 Schema and docs

Every migrated field's schema description is rewritten. The `!`/`!!`/`\!` sentences on discovery, generator, and projection fields are deleted and replaced with a description of the bare/list/`include`+`exclude` forms and the exclude-wins rule. The cache field descriptions gain the same three-form description. Because the schema is generated from schemars doc comments, this is a doc-comment edit per field plus the `oneOf` that `GlobConfig`'s untagged derive produces.

## 6. Mandatory analysis

### 6.1 Existing mechanisms (reuse vs. extend vs. add)

Mostly reuse. `GlobConfig` composes `SingleOrMany<T>` and `MergeGlobConfig` composes the new `MergeSingleOrMany<T>` (a `ListConfig` plus a bare scalar). The compiled sets are RFC 0008's `include_set`, `GlobMatcher` keeps its two-set core from RFC 0007, and the normalized `GlobPatterns<T>` is just an owned include/exclude pair. The genuinely new pieces are `GlobConfig`, `MergeGlobConfig`, `MergeSingleOrMany`, `GlobPatterns`, and `ToGlobPattern`; everything else is a rewiring. The net code count still goes down, because `scan_markers`, `is_include`, the `\!` escape, and RFC 0009's `classify` all disappear.

### 6.2 Maintenance cost

Lower after the migration. Two config types (`GlobConfig`, `MergeGlobConfig`) over one normalized `GlobPatterns` replace four field shapes (`ListConfig<OmniPath>`, `Vec<String>`, `Vec<PathBuf>`, `SingleOrMany<String>`), and one meaning of exclusion replaces three. The lasting obligations: a new glob config field picks `GlobConfig` or `MergeGlobConfig` by whether it layers, and the collector/SCM-filter lockstep from RFC 0009 still holds (both normalize the same config into `GlobPatterns`). The one-time cost is the migration itself, across the config and consumer crates and the in-repo configs, plus rewriting every affected schema description.

### 6.3 Testability

- **Golden hash (the acceptance bar for hashed fields).** RFC 0008's `tests/omni-tests/src/hash-golden.e2e.spec.ts` snapshot must stay byte-identical, because its cache fields are pure-include and migrate to `MergeGlobConfig::Many` with no change in resolved set. This proves the type swap moved no cache key.
- **Round-trip and shape.** For both `GlobConfig<T>` and `MergeGlobConfig<T>`, assert a scalar deserializes to `Single`, a sequence to `Many`, and `{include, exclude}` to `IncludeAndExclude`. For `MergeGlobConfig`, also assert `{append: [...]}` and the other `MergeSingleOrMany` forms parse and layer. Assert each serializes back, and that an object with an unknown key is rejected.
- **Merge layering.** Assert a workspace `MergeGlobConfig` with `include: {append: [a]}` under a project `include: {append: [b]}` yields `[a, b]`, matching `ListConfig` today, and that a `GlobConfig` merge is last-writer-wins.
- **Exclude wins, unordered.** A fixture with `include: ["src/**"]`, `exclude: ["src/gen/**"]` asserts the resolved set omits the excluded subtree, that adding `src/gen/x.rs` back to `include` does not re-include it (unordered, exclude wins), and that two runs agree.
- **Literal `!` needs no escape.** A file literally named `!keep.txt` matched by `include: ["!keep.txt"]` is selected, on Windows and Unix, with no `\!`. This is the direct proof the escape is gone and nothing regressed.
- **Collector and SCM filter agree.** A table-driven test normalizes one config into `GlobPatterns` for both and asserts identical include/exclude behavior, the RFC 0009 lockstep guard carried forward.
- **Per-subsystem migration.** Discovery, generator, and projection suites keep their existing coverage; the `!`-negation cases are rewritten as `include`/`exclude` cases, and any test asserting `!` negates is deleted (the behavior is gone).

Hard to test directly: the collector's `is_match_candidate` fast path is exercised through collection, covered end to end by the golden hash and the exclude fixtures.

### 6.4 Integration points

- New `omni_glob_config` crate: `GlobConfig<T>`, `MergeGlobConfig<T>`, their untagged deserializers and `Merge` impls, and the normalize-to-`GlobPatterns` step. Depends on `omni_glob` (for `GlobPatterns<T>` and `ToGlobPattern`) and `omni_config_types` (`SingleOrMany` and `MergeSingleOrMany`).
- `omni_glob`: owns `GlobPatterns<T>` and `ToGlobPattern`; `GlobMatcher` gains `from_globs`; `scan_markers`/`is_include`/`\!` are removed. `include_set` is unchanged.
- `omni_config_types`: new `MergeSingleOrMany<T>` beside `SingleOrMany`, reusing `config_utils::ListConfig`'s `Merge`. This adds a `config_utils` dependency to `omni_config_types`, a one-directional edge, since `config_utils` does not depend on `omni_config_types`.
- `omni_configurations`: `cache.key.files`, `cache.output.files`, workspace `projects` change type.
- `omni_generator_configurations`: add-many / transform-many `files` change type.
- `omni_projection_configurations`: `match`, `scope` change type; the `validate_match_patterns` deserializer that rejected malformed `!` runs is simplified or removed.
- `omni_collector`, `omni_execution_plan`, `omni_cache`: normalize the config into `GlobPatterns` instead of reading a flat list; the collector and SCM filter as in Section 5.4.
- Schema: every migrated field description.

### 6.5 Potential regressions (highest stakes in the workspace)

- **Cache-key movement on the hashed fields (severe, guarded).** Migrating `cache.key.files` / `cache.output.files` from `ListConfig<OmniPath>` to `MergeGlobConfig<OmniPath>` must not change the resolved set for a pure-include config. Guarded by the golden hash. `MergeGlobConfig` uses `MergeSingleOrMany`, which carries `ListConfig`'s `append`/`prepend`/`replace`/`merge` forms, so a cache field that used those object forms keeps parsing and layering as before.
- **Silent semantic flip for `!` users (intended breaking change).** Any discovery, generator, or projection config that used `!` to exclude now reads that `!` as a literal pattern character and matches nothing (or a file literally named `!x`). This is the breaking change, taken on purpose. Migration is mechanical (move the `!` entries into `exclude`), and a changelog note plus a schema that no longer documents `!` steer authors to the new form. A grep of in-repo configs counts how many are affected before merge.
- **Merge behavior per field.** Cache fields take `MergeGlobConfig` (layered, `ListConfig` semantics), so their existing append/prepend layering is preserved. The non-layering fields take `GlobConfig` (last-writer-wins), which matches how `SingleOrMany` and a bare `Vec` behave for those fields today. No field silently changes its layering rule.
- **Walk-seeding pollution.** As in RFC 0009: an exclude must never seed the walk. Structurally prevented because `include` and `exclude` are separate lists and only `include` feeds `includes`/`match_bases`/`forced_includes`/`topmost`.
- **Cross-platform paths.** `ToGlobPattern` normalizes path separators to `/`; there is no leading-marker parsing left to differ across platforms.

### 6.6 Pros and cons

| Option                                                          | Pros                                                                                                                                                                                                                                                              | Cons                                                                                                                           | Verdict |
| --------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ | ------- |
| **Structured `GlobConfig<T>`, drop `!` (recommended)**          | One type family and one meaning workspace-wide; exclusion is explicit and unordered, matching what omni implements and what Bazel/Rollup/tsconfig do; deletes the `!` scan, the `\!` escape, and RFC 0009's classifier; a flat list can no longer imply gitignore | Breaking config change across several crates; a one-time migration of in-repo configs and every schema description             | ✅      |
| Keep the flat list, add `!` negation to cache fields (RFC 0009) | Smaller diff; no new type                                                                                                                                                                                                                                         | Entrenches the gitignore-implying syntax omni does not honor; leaves `!` meaning three different things; keeps the `\!` escape |         |
| Structured type, but keep `!` as sugar in the list forms        | Familiar to gitignore users                                                                                                                                                                                                                                       | Reintroduces the exact ambiguity this RFC removes; two ways to say exclude                                                     |         |
| Separate `*_exclude` config keys (Bazel-literal)                | No new enum; explicit                                                                                                                                                                                                                                             | A second key per field; loses the terse bare-list common case; more schema to maintain                                         |         |

### 6.7 Prior art

The build tools all use two explicit inputs, unordered, exclude-wins, which is exactly `GlobConfig::IncludeAndExclude`.

| Tool                                           | Their approach                                                                  | What omni takes                                                                                                                                                  |
| ---------------------------------------------- | ------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Bazel `glob(include, exclude)`                 | Two named lists; a file is included if it matches an `include` and no `exclude` | The exact unordered exclude-wins rule, folded into one field with a terse include-only shorthand                                                                 |
| Rollup / Vite `createFilter(include, exclude)` | Two pattern params; id must match an include and no exclude                     | Same rule; confirms this is the norm for declarative file selection, not just Bazel                                                                              |
| tsconfig, webpack loaders                      | Separate `include`/`exclude`                                                    | The two-input shape as the honest model for a config that is not walked top-down                                                                                 |
| gitignore, Biome (`biome_glob`)                | One list, last-match-wins, `!` re-includes, `\!` escapes                        | Deliberately not adopted: that ordered model is for ignore files walked in order, which omni is not. omni drops `!` rather than imply an order it does not honor |

The improvement over the JS tools is that omni carries the same rule into a hashed path under the golden-hash discipline: a pure-include migration is byte-identical, and the SCM-affected filter is held equal to what the collector hashes.

## 7. Open questions

None. The merge model, the one item previously deferred, is resolved by the two-type split (decisions 10 to 13): a field that layers today takes `MergeGlobConfig` (each side a `MergeSingleOrMany`, so `append`/`prepend`/`replace`/`merge` still layer a project onto a workspace default), and a field that does not takes `GlobConfig` (last-writer-wins). Both normalize to `omni_glob::GlobPatterns<T>` after merge. `MergeSingleOrMany` lands in `omni_config_types` beside `SingleOrMany` (decision 14); it depends on `config_utils` for `ListConfig`, a one-directional edge that adds no cycle. `config_utils` must stay free of any `omni_config_types` dependency to keep it that way.

## 8. Decision log

| #   | Decision                                                                                                                                                        | Rationale                                                                                                                                                                                                                          |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Replace the flat glob list with a structured `GlobConfig<T>` across all glob config fields                                                                      | A list implies gitignore last-match-wins, which omni does not do; the same `!` means three different things across fields; a structured type states include vs exclude honestly                                                    |
| 2   | Exclusion only through `IncludeAndExclude`; bare and list forms are include-only                                                                                | A flat list can no longer secretly negate, which removes the ambiguity that started this                                                                                                                                           |
| 3   | Drop the leading-`!` convention and the `\!` escape entirely                                                                                                    | With structural exclusion there is no leading marker, so a literal `!` needs no escape; this reverts the RFC 0008 escape and RFC 0009 classifier, which existed only to serve the `!` syntax                                       |
| 4   | `exclude` always wins, unordered (no re-inclusion)                                                                                                              | Matches omni's existing `GlobMatcher` and Bazel/Rollup/tsconfig; ordered re-inclusion is the ignore-file model omni does not implement                                                                                             |
| 5   | Migrate every subsystem in one change, not just the cache fields                                                                                                | The inconsistency is the problem; a partial migration would leave two conventions live. Doing it once is cheaper than twice                                                                                                        |
| 6   | `omni_glob` owns `GlobPatterns<T>` and `ToGlobPattern`; the new `omni_glob_config` crate owns `GlobConfig<T>` / `MergeGlobConfig<T>` and depends on `omni_glob` | The normalized form and the compile boundary belong with the matcher; the config enums normalize into it. `omni_glob_config` depending on `omni_glob` keeps one direction and no cycle                                             |
| 7   | `ToGlobPattern` translates syntax only; rooted resolution stays with the consumer                                                                               | `@workspace/` resolves against a runtime root map, not known at parse time, so the trait cannot own it                                                                                                                             |
| 8   | Supersede RFC 0009; amend RFC 0005, 0007, 0008                                                                                                                  | 0009's leading-`!` approach is replaced wholesale; the include/exclude split those RFCs built inside `GlobMatcher` survives, only the `!` syntax that fed it is removed                                                            |
| 9   | Accept the breaking config change while pre-release                                                                                                             | Removing the ambiguity is worth a mechanical migration; a changelog note and a schema that no longer documents `!` guide authors                                                                                                   |
| 10  | Resolve the merge model by splitting into two config types instead of one                                                                                       | One type cannot serve both last-writer-wins and `ListConfig` layering cleanly; splitting by which merge a field needs makes each type's `Merge` obvious and total                                                                  |
| 11  | Cache files take `MergeGlobConfig` (layered); discovery, generator, and projection fields take `GlobConfig` (last-writer-wins)                                  | Each field keeps the merge behavior it has today, so the type swap does not change layering; only the cache files layer today (they were `ListConfig`)                                                                             |
| 12  | Both config types normalize to `omni_glob::GlobPatterns<T>` after merge                                                                                         | Consumers compile one owned include/exclude pair regardless of which form or merge type the field used; merge runs on the config representation first so `append`/`prepend` still layer                                            |
| 13  | `MergeSingleOrMany<T>` wraps `ListConfig<T>` and adds only a bare scalar                                                                                        | `ListConfig` already has the list and `append`/`prepend`/`replace`/`merge` forms and their `Merge`; wrapping it rather than copying its variants keeps one merge implementation and adds only the scalar `SingleOrMany` lacks      |
| 14  | `MergeSingleOrMany<T>` lives in `omni_config_types` beside `SingleOrMany`, and `omni_config_types` depends on `config_utils` for `ListConfig`                   | `omni_config_types` sits above `config_utils`; the config value types belong together, and the dependency is one-directional (`config_utils` does not depend on `omni_config_types`), so no cycle forms as long as that stays true |

## 9. Appendix: worked code

Sketches, not final. They mirror the existing `ListConfig` and `SingleOrMany` code so a reviewer can see the shape is buildable with the crates already in the tree. Derives follow the codebase order; the untagged enums hand-write `Deserialize` with `serde_untagged::UntaggedEnumVisitor`, exactly as `ListConfig` does.

### 9.1 `MergeSingleOrMany<T>` (omni_config_types)

```rust
use config_utils::ListConfig;
use merge::Merge;

// A bare scalar, or any ListConfig form. The list and the append/prepend/
// replace/merge layering forms are ListConfig's; this adds only the scalar.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(untagged))]
pub enum MergeSingleOrMany<T: Merge> {
    Single(T),
    List(ListConfig<T>),
}

impl<T: Merge> MergeSingleOrMany<T> {
    pub fn empty() -> Self {
        Self::List(ListConfig::value(Vec::new()))
    }

    pub fn into_vec(self) -> Vec<T> {
        self.into_list().into_vec()
    }

    fn into_list(self) -> ListConfig<T> {
        match self {
            Self::Single(t) => ListConfig::value(vec![t]),
            Self::List(l) => l,
        }
    }
}

// Deserialize: a scalar is Single; a sequence or object is handed to
// ListConfig, which already parses the list and all the merge forms.
#[cfg(feature = "serde")]
impl<'de, T> serde::Deserialize<'de> for MergeSingleOrMany<T>
where
    T: serde::de::DeserializeOwned + Merge,
{
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        serde_untagged::UntaggedEnumVisitor::new()
            .string(|s| {
                serde_json::from_value(serde_json::Value::String(s.to_owned()))
                    .map(MergeSingleOrMany::Single)
                    .map_err(serde::de::Error::custom)
            })
            .seq(|s| s.deserialize().map(MergeSingleOrMany::List))
            .map(|m| m.deserialize().map(MergeSingleOrMany::List))
            .deserialize(d)
    }
}

// Merge delegates to ListConfig: Single becomes a one-element list, then the
// two lists merge with ListConfig's append/prepend/replace/merge rules.
impl<T: Merge + Clone> Merge for MergeSingleOrMany<T> {
    fn merge(&mut self, other: Self) {
        let mut this = std::mem::replace(self, Self::empty()).into_list();
        this.merge(other.into_list());
        *self = Self::List(this);
    }
}
```

### 9.2 `GlobPatterns<T>` and `ToGlobPattern` (omni_glob)

```rust
use std::{borrow::Cow, path::Path};

pub trait ToGlobPattern {
    fn to_glob_pattern(&self) -> Cow<'_, str>;
}

impl ToGlobPattern for String {
    fn to_glob_pattern(&self) -> Cow<'_, str> { Cow::Borrowed(self) }
}

impl ToGlobPattern for Path {
    fn to_glob_pattern(&self) -> Cow<'_, str> {
        let s = self.to_string_lossy();
        if s.contains('\\') { Cow::Owned(s.replace('\\', "/")) } else { s }
    }
}

// OmniPath yields its display form; the consumer still resolves @workspace/
// against the root map before matching.
impl ToGlobPattern for omni_types::OmniPath {
    fn to_glob_pattern(&self) -> Cow<'_, str> { Cow::Owned(self.to_string()) }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GlobPatterns<T> {
    pub include: Vec<T>,
    pub exclude: Vec<T>,
}

impl<T: ToGlobPattern> GlobPatterns<T> {
    /// Pattern strings for the two sides, ready to hand to include_set or
    /// GlobMatcher::from_globs. Rooted resolution, when T is OmniPath, happens
    /// in the consumer before this call.
    pub fn to_pattern_strings(&self) -> (Vec<String>, Vec<String>) {
        let render = |xs: &[T]| xs.iter().map(|t| t.to_glob_pattern().into_owned()).collect();
        (render(&self.include), render(&self.exclude))
    }
}
```

### 9.3 `GlobConfig<T>` and `MergeGlobConfig<T>` (omni_glob_config)

```rust
use merge::Merge;
use omni_config_types::{MergeSingleOrMany, SingleOrMany};
use omni_glob::{GlobPatterns, ToGlobPattern};

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum GlobConfig<T> {
    Single(T),
    Many(Vec<T>),
    IncludeAndExclude { include: SingleOrMany<T>, exclude: SingleOrMany<T> },
}

impl<T> GlobConfig<T> {
    pub fn normalize(self) -> GlobPatterns<T> {
        match self {
            Self::Single(t) => GlobPatterns { include: vec![t], exclude: vec![] },
            Self::Many(v) => GlobPatterns { include: v, exclude: vec![] },
            Self::IncludeAndExclude { include, exclude } => GlobPatterns {
                include: include.into_vec(),
                exclude: exclude.into_vec(),
            },
        }
    }
}

// GlobConfig merges last-writer-wins, matching SingleOrMany.
impl<T> Merge for GlobConfig<T> {
    fn merge(&mut self, other: Self) { *self = other; }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum MergeGlobConfig<T: Merge> {
    Single(T),
    Many(MergeSingleOrMany<T>),
    IncludeAndExclude { include: MergeSingleOrMany<T>, exclude: MergeSingleOrMany<T> },
}

impl<T: Merge> MergeGlobConfig<T> {
    pub fn normalize(self) -> GlobPatterns<T> {
        let (include, exclude) = self.into_sides();
        GlobPatterns { include: include.into_vec(), exclude: exclude.into_vec() }
    }

    // Every form as an (include, exclude) pair. Single/Many have an empty
    // exclude, so a cross-variant merge is always a per-side merge.
    fn into_sides(self) -> (MergeSingleOrMany<T>, MergeSingleOrMany<T>) {
        match self {
            Self::Single(t) => (MergeSingleOrMany::Single(t), MergeSingleOrMany::empty()),
            Self::Many(m) => (m, MergeSingleOrMany::empty()),
            Self::IncludeAndExclude { include, exclude } => (include, exclude),
        }
    }
}

// MergeGlobConfig merges per side. A cross-variant merge (a Many under an
// IncludeAndExclude) folds the Many into the include side, so the result is
// always a well-defined include/exclude pair.
impl<T: Merge + Clone> Merge for MergeGlobConfig<T> {
    fn merge(&mut self, other: Self) {
        let this = std::mem::replace(self, Self::Many(MergeSingleOrMany::empty()));
        let (mut si, mut se) = this.into_sides();
        let (oi, oe) = other.into_sides();
        si.merge(oi);
        se.merge(oe);
        *self = Self::IncludeAndExclude { include: si, exclude: se };
    }
}
```

The untagged `Deserialize` for both follows `MergeSingleOrMany`'s: a scalar is `Single`, a sequence is `Many`, an object with `include`/`exclude` keys is `IncludeAndExclude`, and (for `MergeGlobConfig`) an object with `append`/`prepend`/`replace`/`merge` keys is `Many(MergeSingleOrMany::...)`. The key set decides, so the two object shapes never collide.

### 9.4 A consumer, end to end

```rust
// Collector: cache.key.files is MergeGlobConfig<OmniPath>, already merged
// across layers. Normalize, resolve each side against the root map, compile.
let patterns: GlobPatterns<OmniPath> = task.cache.key.files.clone().normalize();

let resolve = |ps: &[OmniPath]| -> Vec<String> {
    ps.iter()
        .map(|p| absolute(project_dir.join(p.resolve(roots))).to_string_lossy().into_owned())
        .collect()
};
let include = include_set(&resolve(&patterns.include), GlobOptions::default())?;
let exclude = include_set(&resolve(&patterns.exclude), GlobOptions::default())?;

// match loop, per walked file:
if include.is_match_candidate(&cand) && !exclude.is_match_candidate(&cand) {
    resolved_input_files.push(rooted_path);
}
```

### 9.5 Config forms and how they normalize

```yaml
# bare scalar            -> GlobPatterns { include: ["src/**"], exclude: [] }
files: "src/**"

# list                   -> GlobPatterns { include: ["a", "b"], exclude: [] }
files: ["a/**", "b/**"]

# include + exclude      -> GlobPatterns { include: ["src/**"], exclude: ["src/gen/**"] }
files:
    include: ["src/**"]
    exclude: "src/gen/**" # each side is a SingleOrMany / MergeSingleOrMany

# layered exclude on a cache field (MergeGlobConfig only)
cache:
    key:
        files:
            include: { append: ["src/**"] } # append onto the workspace default
            exclude: ["src/gen/**"]
```

A file literally named `!keep.txt` is reachable as an ordinary pattern, `include: ["!keep.txt"]`, with no escape, because there is no leading-marker parsing left.
