# Tooling

Keeping this up to date: change this page in the same PR that adds, removes or upgrades a tool, and delete stale entries.

## Toolchain

- Rust: nightly pinned in `rust-toolchain.toml` (with `rustfmt`, `clippy` and the `wasm32-unknown-unknown` target). Crates are in `crates/` and `cli/omni`, sharing `Cargo.toml` workspace dependencies.
- TypeScript: bun (pinned in `package.json` `packageManager`) with workspaces in `packages/`, `scripts/`, `apps/`, `services/` and `tests/`. Dependency versions live in the root catalog.
- Docs site: `apps/site`.
- Cache output replay uses `cap-std` and `cap-fs-ext` (Bytecode Alliance,
  Apache-2.0/Apache-2.0-with-LLVM-exception/MIT) behind the executor's private
  `cache_restore` module. Directory-relative no-follow operations and retained
  Windows handles avoid implementing platform security primitives ourselves;
  dependency versions are locked in `Cargo.lock`.
- The captured input view (`omni_input_view`, RFC 0024 F10) owns its containment
  primitive instead: on Linux a direct `rustix::fs::openat2` (workspace
  dependency `rustix`, features `std` and `fs`) with `RESOLVE_BENEATH |
  RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS | RESOLVE_NO_XDEV` and no fallback
  (`cap-std` falls back silently to a userspace walk, so it is not used there).
  On Windows it uses `cap-std` handle-relative opens in its own one-component
  walk plus one contained `unsafe` call to `GetFinalPathNameByHandleW` through
  `windows-sys`. Other OSes, macOS included, are `Unsupported`.

## Quality tools

`omni_setup` uses the Rust multiplatform test preset so its fallback-keystore
access-control tests run on native Windows CI. Run them locally on Windows
with `cargo test -p omni_setup fallback_store::key_file`.

`omni_input_view` uses the same preset because its containment tests must pass
on native Linux, Windows and macOS runners. Windows symlink tests follow
`OMNI_TEST_REQUIRE_CAPABILITIES` (they fail on CI when the privilege is missing
and skip only locally); junction tests never skip. Run it locally with
`cargo test -p omni_input_view`.

- Rust: `cargo fmt`, `cargo clippy`, `cargo test`; benchmarks use criterion (`crates/omni_bench`).
- TypeScript: `oxfmt` (`oxfmt.config.ts`), `oxlint` (`oxlint.config.ts`), `tsc`, `vitest` (`vitest.workspace.ts`).

## Dev tools

- `omni`: the task runner used on this repo. Tasks come from `omni/presets/*.omni.yaml`; run `omni run <task> -p <project>`.
- Wasm builds: set `WASM_PACK_ADDITIONAL_ARGS=--no-opt` for wasm-pack builds (extra arguments are appended to the command; it avoids a `sh` wrapper, which on Windows puts Git's `link.exe` ahead of MSVC's) or `WORKER_BUILD_NO_OPT=true` for Cloudflare worker builds (for example in the project's `.env.local`) to skip wasm-opt. Optimization remains enabled by default.
- `omni generate`: scaffolds from `generators/` (`crate`, `rust-error`, `ts-package`).
- `omni remote-sources install` and `omni projection sync`: fetch and link the projected guidelines and skills (`workspace.omni.yaml`).
- `cog` (cocogitto): commit checks and releases, configured in `cog.toml`.
- `docker`: available for container-based tests.

## Remote-cache worker tests

`omni run test:service:worker -p @omni-oss/omni-remote-cache-service-tests`
builds the worker and runs the API suite through local Wrangler/Miniflare.
Node must be on `PATH`: without it, `bunx wrangler` can fall back to Bun,
which Wrangler rejects with HTTP 503 because Bun's `fetch` does not support
the dispatcher Miniflare uses.

On a failing worker test, the harness prints captured Wrangler output and
process exit information. The 20 MiB upload assertion also reports the response
body on failure. These diagnostics do not retry the request or weaken the
expected status.
