# omni_migration

Storage-agnostic planner and runner for pre-v1 store migrations, from [RFC 0032](../../docs/rfc/0032-pre-v1-migration-infrastructure.md). **Unwired:** no Omni crate depends on it and it changes no existing store. It is not production-ready; see the RFC for what is and is not proven.

`src/` is the contract: chain validation and digest, the sealed `Plan` and planner, `Policy`, backend capability checks, the zero-work `gate_access`, and the `execute` runner with its adapter traits. It has no storage, async runtime or Omni dependency.

`tests/common/` holds the two reference fixtures (an atomic lockfile and a non-atomic index) and a fault-injecting filesystem. They are test-only and not part of the API. The acceptance cases run in `tests/`, including subprocess kill and a barrier race.

Benchmark: `cargo bench -p omni_migration --bench access_gate`.
