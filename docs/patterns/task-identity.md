# Task identity

A task is its raw `(project, task)` pair. The joined text `project#task` is a
display spelling, never an identity.

- Use `omni_types::TaskKey` (owned, cheap to clone) and `TaskKeyRef` (borrowed).
  Key maps with `omni_types::TaskMap<V>`: it probes with `&str` components and
  builds no key or label. Never use a `String` label as a map key, a set member
  or a dependency edge.
- Render a label only where data leaves the process (CLI text, JSON, MCP,
  notices, logs) with `omni_types::task_label` or `TaskKey::label()`. The label
  is escaped with the task-reference grammar, so distinct pairs render distinct
  labels. Do not write `format!("{}#{}", ...)` for a task label, and never
  split a label on `#` to recover a project or task.
- Parse text only at ingress (exact selectors, dependency text), with
  `TaskReference`. Empty or NUL project and task names are rejected when the
  workspace loads (`workspace.invalid_project_name`, `workspace.invalid_task_name`).
- Executable nodes (`TaskExecutionNode`) come only from the graph builder and
  the `omni exec` and `--ignore-dependencies` paths in `omni_execution_plan`.
  Serialized forms are reports (`TaskExecutionNodeReport`,
  `TaskExecutionGraphReport`, `TaskExecutionResultReport`) and are never
  executed.
- The cache key contributes the identity through
  `identity_leaf_v1` in `crates/omni_collector/src/identity_leaf.rs`
  (length-prefixed project and task bytes). Changing that recipe invalidates every task cache; version it and
  add pinned vectors.
- Registries that must route by an object rather than a name (for example the
  terminal presenter's streams) use a unique handle, not a label.
