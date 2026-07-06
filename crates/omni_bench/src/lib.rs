//! In-process benchmark harness for omni's task-execution pipeline.
//!
//! This crate hosts the criterion benchmarks that measure the cost of
//! [`omni_task_executor::TaskExecutor::run`] end to end on realistic, generated
//! workspaces. See `docs/rfc/0004-task-execution-benchmarking.md` for the
//! design and rationale.
//!
//! Workspaces are generated on disk (into a `TempDir`) by
//! [`omni_test_utils`]; generation and project discovery happen **outside** the
//! measured region. A given preset produces a byte-identical workspace every
//! run (seeded graph + ordered maps), so the benchmarks measure code changes,
//! not workload drift.

pub mod harness;
pub mod matrix;
