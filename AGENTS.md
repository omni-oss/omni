# Agent instructions

Follow the guidelines in `docs/guidelines`. They are projected from swe-docs and not tracked, so in a fresh checkout run `bun install && omni remote-sources install && omni projection sync` first.

## Artifacts

- Design and plans: `docs/rfc/`.
- Requirements: `docs/requirements/`.
- User docs (tutorials, guides, reference, release notes): `apps/site` (content in `apps/site/content`). Keep them apart from `docs/guidelines`.

## Checks

Tasks run through omni and are defined in `omni/presets/*.omni.yaml`; CI runs them from `.github/workflows/ci.yaml`. Scope to the project you changed: `omni run <task> -p <project>`.

| Task | Command | Covers |
| --- | --- | --- |
| `check:lint` (Rust) | `cargo clippy -p <crate> -- -D warnings` | Rust crates |
| `check:format` (Rust) | `cargo fmt -p <crate> -- --check` | Rust crates |
| `test` (Rust) | `cargo test -p <crate>` | Rust crates |
| `check:format` (TS) | `bun x oxfmt --check` | TS projects |
| `check:lint` (TS) | `bun x oxlint <project>` | TS projects |
| `check:types` (TS) | `bun x tsc --noEmit -p tsconfig.json && if [ -f tsconfig.node.json ]; then bun x tsc --noEmit -p tsconfig.node.json; fi` | TS projects |
| `test:unit`, `test:integration` (TS) | `bun run test:unit`, `bun run test:integration` | TS projects |
| `build` | `bun run build` (TS), cargo build (Rust) | Per project; `cli/omni` lint needs `@omni-oss/bridge-service#build` first |

Whole-workspace runs and cross-target builds (`build:<target>`) are slow; leave them to CI.

## Risk overrides

These areas are always High risk (`docs/guidelines/core/precedence.md`):

- Caching (`crates/omni_cache`, remote cache crates): rule: Low/Medium risk shortcuts; action: changes to cache keys, storage or invalidation; reason: wrong hits or stale output silently corrupt builds; effective rule: treat as High.
- Directory walking (`crates/dir_walker`, ignore and glob crates): reason: misses or extra files change hashes and what gets processed; effective rule: High.
- Capabilities (`crates/omni_capabilities`, `omni_capability_enforcement`, `omni_capability_sys`): reason: security boundary for what tasks may do; effective rule: High.

## Agent choices

If `.agents/agent-choices.local.md` exists, read it before delegating work and use the agents it names.
