# F03: Structured scoped task identity and root-based references

**Type:** feature · **Size:** L · **Depends on:** R05, R06 · **Unblocks:** F04

Audit prerequisite: [F03.1 identity and parser inventory](F03.1-identity-parser-audit.md).
The escape-aware parser/formatter ([F03.2 part 1](F03.2-reference-parser.md)) is
merged and now parses `dependencies`/`with` ([part 2](F03.2-reference-parser.md#part-2-wiring-into-dependencies-and-with)); root lowering
(F03.3) is implemented on this branch and awaits review; plugin-scoped variants remain unimplemented. F03.1 ([#103](https://github.com/omni-oss/omni/pull/103)),
F03.2 part 1 ([#116](https://github.com/omni-oss/omni/pull/116)) and part 2 ([#117](https://github.com/omni-oss/omni/pull/117)) are merged.

## Problem

`TaskDependencyConfiguration` is string-only. A literal local task named `native::build`
cannot be told apart from the scoped pair `(native, build)`. Task `dependencies`/`with`
reference a project **name**, so a reference breaks when a project is renamed.

## Goal

1. A structured task identity that preserves raw components (optional `plugin`, `project`
   or `project_root`, raw `task`) and one shared full-input parser/formatter.
2. A `ProjectRoot` reference variant for `dependencies` and `with`, lowered to final
   project names after composition and task inheritance.
3. Handwritten shorthand `project#task` and `{ project, task }` objects.

(Parts using `plugin` are inert until plugins exist; implement the schema and parsing
now, with tests using a fake qualifier source.)

## RFC references

- [§17.3](../../017.00-review-findings-and-prerequisites.md#173-root-based-references-for-task-dependency-fields)
- [§17.8](../../017.00-review-findings-and-prerequisites.md#178-scoped-object-references-and-alias-aware-selectors)
- [§7.2](../../007.01-layer-order-and-merge-semantics.md), [§7.4](../../007.03-task-scoping-aliases-and-selectors.md)
- C08, C14, C15, C20

## Code starting points

- `crates/omni_configurations/src/task_dependency_configuration.rs`
- `crates/omni_execution_plan/src/call.rs`, `filter.rs`
- Identity-keyed maps: `omni_core::TaskKey` (project/task only), task context and
  override-argument lookups; cache CLI parsing in `omni_cli_core/src/commands/cache.rs`
- Shorthand `project#task` strings already exist for explicit projects.

## Work

1. **Audit** every identity-keyed map and parser (planner requests, `TaskKey`, context,
   argument overrides, cache CLI) and list required changes.
2. Define the strict object schema; never reinterpret a raw component as shorthand;
   escape/round-trip rules; schema parity.
3. Add the lowering step after task extensions and before executable dependency
   conversion and final target validation. Keep replacement-before-validation, provenance
   and malformed-path checks.
4. Keep the one-canonical-task-one-binding execution model (§17.10): no new invocation
   key.

## Out of scope

- Plugin-emitted references; alias selection (F04); activation binding.

## Acceptance

- Rename of a target project leaves root-based references intact.
- Literal `native::build` task name round-trips and is not scoped.
- `with` and upstream references, removed target tasks, explicit replacement.
- Scoped/literal object and shorthand round-trip tests; schema parity.

## F03.3 implementation slice

Scope approved: internal root references only. The handwritten schema, alias
resolution, and plugin scopes and bindings are unchanged. This is Medium-sized
work across configuration and context, with High risk because it chooses
executable dependency targets. One implementer owns the slice; independent review
is required before calling it done.

Approved scope expansion: the first review found that a `replace` in one task
declaration was undone by a later `append` once the task inherited a template or
the project settings. This predates F03.3 and also affected a task's `args`,
`meta`, `env.vars` and cache lists. The slice composes tasks as ordered
`TaskDeclarations`, not as merged fields. Task `extends` adds the template's
declarations to that sequence. `finalize_composed` folds the sequence into
task settings and the effective `cache` and `meta` over project settings.
The declaration sequence is internal and has no handwritten encoding.
`ListConfig::merge` and project `extends` traversal are unchanged.

1. A typed `ProjectRootId` variant carries the reference through the existing
   task lists. It has no handwritten encoding.
2. Lowering runs once per extraction, before any environment is loaded, against
   the finalized runnable projects. A missing task, a missing, non-runnable or
   ambiguous root fails with the field, the consuming task, the declaring task
   and its source file if available, the target root and the reason. A reference
   removed by replacement is never looked up.
3. Task `meta` is now computed in `finalize_composed` (`FinalizedProject::task_meta`)
   so project `meta` is applied once.

Rust callers should handle `TaskConfiguration::get_task` as a `Result` or pass
a resolver to `TaskConfigurationLongForm::into_task` when a project catalog is
available. The infallible `From<TaskDependencyConfiguration>` conversion is no
longer available. `TaskLayer` also has a new `Composed` variant, and
`FinalizedProject` has a new `task_meta` field. This does not change handwritten
configuration.

Verified by `crates/omni_configurations/tests/inherited_operations.rs` and the extractor
tests in `crates/omni_context/src/project_data_extractor.rs`, including a cycle
closed through a lowered reference in the real task graph.

Deferred, with the reason:

- Public-name lookup. The resolver checks `config.tasks`; plugin-origin
  references must bind to the originating activation instead.
- Serializing a finalized config that still holds a root reference. Nothing
  produces one yet; `ProjectConfigResponse` fails on it until a producer exists.
  Tracked in [#141](https://github.com/omni-oss/omni/issues/141) and in P04.3's
  acceptance criteria.
- Cache identity. Cache keys do not cover `meta` or unkeyed environment variables.
  [F06.3](F06-effective-semantic-digest.md) owns the semantic digest. The user guide documents a fresh keyed environment
  value to avoid stale local and remote entries on upgrade and rollback.
- The eager `resolve_task_extensions` path (`ProjectConfiguration::load`) still
  merges before inheriting. The loader uses the layer path. Tracked in
  [#142](https://github.com/omni-oss/omni/issues/142).
- `args` key order is random after a dictionary merge and is hashed unsorted
  (`omni_collector/src/collector.rs`). This predates the slice and is not yet
  reproduced. Tracked in [#140](https://github.com/omni-oss/omni/issues/140).
- `ExtensionGraph` can apply a descendant before its ancestor, so `extends` order
  can undo an override. Tracked in
  [#139](https://github.com/omni-oss/omni/issues/139).

The type and its lowering land together. Undo both together and use a fresh
cache revision value for affected tasks on rollback.
