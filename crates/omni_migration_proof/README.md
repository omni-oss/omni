# omni_migration_proof

Disposable slice 1 harness for [RFC 0032](../../docs/rfc/0032-pre-v1-migration-infrastructure.md). Nothing depends on it and it touches no production store. Delete the crate to undo.

It holds the shared planner/runner (`core`), a fault-injecting durable filesystem (`fs`) and the two fixtures (`lockfile`, `index`). The acceptance cases run in `tests/`, including subprocess kill and barrier tests.

## Not covered in this slice

- Allocation and event counting on the zero-work path, and the `black_box` benchmark (slice 3).
- Async cancellation (dropping an awaiter). The fixtures are synchronous, as the RFC prescribes first.
- Power loss, network filesystems and non-Linux platforms. Process kill does not simulate power loss; persistence failures are simulated by failpoints.
