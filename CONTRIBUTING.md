# Contributing

Keeping this up to date: this page describes how the repo works today. Change it in the same PR as the change that makes it wrong (a new command, tool or workflow), and fix or delete stale entries.

See also [tooling](docs/tooling.md) and [patterns](docs/patterns/README.md).

## Checks

Tasks run through omni and are defined in `omni/presets/*.omni.yaml`. CI runs them from `.github/workflows/ci.yaml`. Scope a run to the project you changed: `omni run <task> -p <project>`.

| Task | Command | Covers |
| --- | --- | --- |
| `check:lint` (Rust) | `cargo clippy -p <crate> -- -D warnings` | Rust crates |
| `check:format` (Rust) | `cargo fmt -p <crate> -- --check` | Rust crates |
| `test` (Rust) | `cargo test -p <crate>` | Rust crates |
| `check:format` (TS) | `bun x oxfmt --check` | TS projects |
| `check:lint` (TS) | `bun x oxlint <project>` | TS projects |
| `check:types` (TS) | run from the project directory: `bun x tsc --noEmit -p tsconfig.json && if [ -f tsconfig.node.json ]; then bun x tsc --noEmit -p tsconfig.node.json; fi` | TS projects |
| `test:unit`, `test:integration` (TS) | `bun run test:unit`, `bun run test:integration` | TS projects |
| `build` | `bun run build` (TS), cargo build (Rust) | Per project |

- Vite projects (for example `@omni-oss/channels`) have no `test:unit` or `build` package scripts; their presets run `vitest` and `bun x vite build` directly. Prefer the scoped omni task (`omni run <task> -p <project>`) for TS projects.
- `cli/omni` lint needs `@omni-oss/bridge-service#build` first.
- Slow: whole-workspace runs and cross-target builds (`build:<target>`). Leave these to CI.
- Only CI runs: cross-target builds, publishing, release and perf-dashboard workflows (`.github/workflows`).

## Git

- Default branch: `main`. Work on a branch named `<type>/<short-slug>` (for example `feat/...`, `fix/...`, `chore/...`, `docs/...`, `refactor/...`, `ci/...`) and open a PR into `main`.
- Commits mostly follow Conventional Commits (`fix: ...`, `feat!: ...`). `cog.toml` configures `cog` and defines a `commit-msg` hook that runs `cog verify` and `cog check`; installing it is optional and personal.
- PRs are merged with merge commits (`Merge pull request #N`). `.github/workflows/check-pr-message.yaml` runs `cog check` on pushes, so commit messages (not PR titles) must pass it.
- CI (`ci.yaml`) runs on pushes to `main`, on PRs to `main` and in the merge queue. PRs run quick CI (checks and TypeScript tests); Rust build and tests run in the merge queue and on `main` (see `docs/rfc/0029-integration-branch-workflow.md`).
- No repo-managed git hooks, PR template or CODEOWNERS.
- Never force-push shared branches or commit secrets.
