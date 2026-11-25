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
- Parse text only at ingress, with `TaskReference`: dependency text, declaration
  keys, `extends` operands and alias targets (decoded when the layer or the
  composed project is read), and live exact selectors
  (`TaskSelector::parse_exact`, called once by `omni run`, `hash project`,
  `cache eligibility` and the matching API and MCP handlers). Wildcards in an
  exact selector are literal; patterns never go through the reference parser.
  Never split a label on `#`. Empty or NUL project and task names are rejected
  when the workspace loads (`workspace.invalid_project_name`,
  `workspace.invalid_task_name`).
- An alias is another name for a task of the same project, not a task. The
  composed project carries `TaskAliases` (alias to canonical name, chains
  flattened); resolve a public name with `Project::canonical_task_name` and ask
  `Project::is_alias`. Resolve public names to the canonical `(project, task)`
  before creating an edge, a result, a hash input or a cache name, and never store
  or hash an alias. `TaskSelector::Exact` is a raw canonical name (inventory,
  internal names); `TaskSelector::Reference` resolves aliases.
- Planning returns an `ExecutionSelection`: `direct` (what the selectors matched)
  and `plan` (the expansion). Fold only `direct` into an aggregate such as a
  project hash.
- Stored-name callers (cache stats, prune and stale inventory) plan with
  `with_raw_canonical_names(true)`, and use direct matches for `--dir` and
  `--meta`. An alias miss there is advice only (`CacheAliasHint`); it never
  changes a selection, an approved path or a confirmation.
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
