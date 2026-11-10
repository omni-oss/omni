# F03: Structured scoped task identity and root-based references

**Type:** feature · **Size:** L · **Depends on:** R05, R06 · **Unblocks:** F04

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
