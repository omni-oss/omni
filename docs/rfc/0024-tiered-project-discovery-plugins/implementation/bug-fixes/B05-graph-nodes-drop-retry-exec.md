# B05: Graph-built task nodes drop `retry_exec`

**Type:** bug fix · **Risk:** High (cache keys) · **Status:** approved, implemented

## Problem

`TaskExecutionGraph::from_project_graph` builds `TaskExecutionNode` with
`task_retry_exec = None` (`crates/omni_core/src/task_execution_graph.rs` ~L346).
The `--ignore-dependencies` path (`execution_plan_provider.rs` ~L241) passes
`task.retry_exec.clone()`. The executor reads the node
(`batch_executor.rs` ~L488), so `retry_exec` only runs with `--ignore-dependencies`.

## Cache impact (why High)

`retry_exec` is a cache-key input (`omni_collector` `retry_command` leaf).
`ProjectHasher` reads it from `project.tasks`, but the executor path builds its
`TaskExecutionInfo` from the node (`omni_cache/src/exts.rs`), so on the graph path
the executor-side digest omits it and cached entries record `task_retry_exec: None`.
Passing the real value changes that digest for every task with `retry_exec`: a one-time
miss for those tasks, after which keys agree with the hasher and the
`--ignore-dependencies` path. RFC 0028 states `retry_exec` is hashed.

## Proposed fix

Pass `task.1.retry_exec.clone()` in `from_project_graph`. Nothing else changes.

## Proof (before building)

- Unit test in `omni_core`: graph node carries the task's `retry_exec`.
- e2e (node marker scripts, no shell syntax): task fails once, has `retry_exec`;
  run with and without `--ignore-dependencies`; marker proves it runs in both.
- e2e: with caching on, a second run hits and changing `retry_exec` misses on the normal path. A direct executor/`ProjectHasher` digest-parity test is not added; F06.3's key-parity tests cover it once merged.

## To confirm first

- Reproduced with a debug build: default run fails, `--ignore-dependencies` retries via `retry_exec`.
- Interaction with unmerged `feat/rfc0024-f06-3-digest-in-key` (reads command/retry
  from the plan node; this fix makes the node correct for it). Rebase if it lands first.

## Release

User-visible: `retry_exec` starts running on normal runs; release note. Rollback:
revert the one-line change (keys revert too).
