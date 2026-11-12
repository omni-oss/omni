# Tooling

Keeping this up to date: change this page in the same PR that adds, removes or upgrades a tool, and delete stale entries.

## Toolchain

- Rust: nightly pinned in `rust-toolchain.toml` (with `rustfmt`, `clippy` and the `wasm32-unknown-unknown` target). Crates are in `crates/` and `cli/omni`, sharing `Cargo.toml` workspace dependencies.
- TypeScript: bun (pinned in `package.json` `packageManager`) with workspaces in `packages/`, `scripts/`, `apps/`, `services/` and `tests/`. Dependency versions live in the root catalog.
- Docs site: `apps/site`.

## Quality tools

- Rust: `cargo fmt`, `cargo clippy`, `cargo test`; benchmarks use criterion (`crates/omni_bench`).
- TypeScript: `oxfmt` (`oxfmt.config.ts`), `oxlint` (`oxlint.config.ts`), `tsc`, `vitest` (`vitest.workspace.ts`).

## Dev tools

- `omni`: the task runner used on this repo. Tasks come from `omni/presets/*.omni.yaml`; run `omni run <task> -p <project>`.
- `omni generate`: scaffolds from `generators/` (`crate`, `rust-error`, `ts-package`).
- `omni remote-sources install` and `omni projection sync`: fetch and link the projected guidelines and skills (`workspace.omni.yaml`).
- `cog` (cocogitto): commit checks and releases, configured in `cog.toml`.
- `docker`: available for container-based tests.
