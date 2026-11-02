# RFC 0019: Cloudflare Workers support for `omni_remote_cache_service`

- **Status:** Accepted
- **Owner:** TBD
- **Date:** 2026-11-02
- **Base branch:** `main`
- **Affected crates:** `omni_remote_cache_service`, `omni_remote_cache_storage_core` (renamed), `omni_remote_cache_storage_s3`, `omni_remote_cache_storage_local`, `omni_remote_cache_storage_memory`, `omni_remote_cache_storage_r2`, `omni_remote_cache_core`, `omni_remote_cache_worker`, CI workflows

## 1. Summary

Target: `wasm32-unknown-unknown` via `workers-rs`, keeping the native (Tokio + S3/local) build.

## Crate layout

`omni_remote_cache_storage` is renamed to `omni_remote_cache_storage_core` and reduced to abstractions and helpers. Each backend becomes its own crate.

| Crate | Purpose |
| --- | --- |
| `crates/omni_remote_cache_storage_core` | Traits, structs, errors, runtime-agnostic helpers (e.g. `LruCached`), test utils. No Tokio/AWS/fs. |
| `crates/omni_remote_cache_storage_s3` | AWS S3 backend (native). |
| `crates/omni_remote_cache_storage_local` | Local disk backend (native, Tokio fs). |
| `crates/omni_remote_cache_storage_memory` | In-memory backend (tests, dev; Tokio-free so it also builds on wasm). |
| `crates/omni_remote_cache_storage_r2` | `worker::Bucket` backend (Workers). |
| `crates/omni_remote_cache_core` | Portable routes, handlers, auth, models. |
| `services/omni_remote_cache_worker` | `cdylib` with the `fetch` entry point, config from env/secrets. |

All new crates are scaffolded empty via the `crate` generator; the rename is done.

## Phase 1: Refactor (do first)

1. Rename `omni_remote_cache_storage` to `omni_remote_cache_storage_core` (done) and update the service.
2. Gate `utoipa` `ToSchema` in `structs.rs` behind the `utoipa` feature.
3. Move `impls/s3.rs` to `omni_remote_cache_storage_s3` and `impls/local.rs` to `omni_remote_cache_storage_local`, with their dependencies (`aws-*`, `http*`, `tempfile`, `tokio`). Remove the `s3`/`local-disk` features from core.
4. Make `LruCached` runtime-agnostic (swap `tokio::sync::Mutex`/`tokio_stream` for `futures-util`/`async-lock`) and stop buffering the full upload in `save_stream`. Move `impls/memory.rs` to `omni_remote_cache_storage_memory` and make it Tokio-free.
5. Decide `Send` strategy: `worker::send::SendFuture`/`#[worker::send]` at the worker boundary, or `#[async_trait(?Send)]` on wasm.
6. Extract `routes`, `security`, `extractors`, `request`, `response`, `data`, `services`, `scalar` from `omni_remote_cache_service` into `omni_remote_cache_core`. Keep `main.rs`, `args.rs`, `init_tracing.rs`, file config loading in the native service, which depends on the s3/local crates.
7. Config abstraction constructible from file (native) or Worker env.
8. Enable `time` `wasm-bindgen`; check `ahash`/`getrandom` in `maps`/`sets`.
9. `axum`/`axum-extra` with `default-features = false` in core; drop `file-stream`/`async-read-body`.

Exit criteria: native service tests unchanged and green; `cargo check -p omni_remote_cache_core -p omni_remote_cache_storage_core --target wasm32-unknown-unknown` passes.

## Phase 2: R2 backend + worker

- Implement the storage trait over `worker::Bucket` with streaming `put`/`get`.
- Worker `#[event(fetch)]` builds the shared Axum router, state from `Env`.
- `wrangler.toml` with R2 binding; `worker-build --release`; local `wrangler dev` smoke test.

## Phase 3: CI plan (wasm targets for Rust)

CI is planned by omni (`omni run test build publish` dry-run -> `jobs_build_rust`/`jobs_test_rust`) and executed by `run-omni-jobs.yaml`. Because the plan is derived from project tasks, wasm checks are expressed as omni tasks and run in the existing `test-rust`/`build-rust` jobs; no new workflow job is required.

Implemented:

1. **Toolchain**: `rust-toolchain.toml` lists `wasm32-unknown-unknown` in `targets`, so `setup-rust-toolchain` and local rustup install it.
2. **Presets**:
   - `omni/presets/rust-wasm-compatible.omni.yaml` adds a `check-wasm` task (`cargo check --target wasm32-unknown-unknown`) and wires it into `test`. Applied to `omni_remote_cache_storage_core`, `_memory`, `_r2` and `omni_remote_cache_core`.
   - `omni/presets/rust-cloudflare-worker.omni.yaml` (worker) adds `check-wasm-deps`, which fails if `mio`, `socket2`, `tokio-util` or `aws-sdk-s3` appear in the wasm dependency graph (guards against feature unification), and a `build` task running `worker-build --release` (installed on demand at a pinned version) with `build/**` cached as output.
3. **getrandom**: `.cargo/config.toml` sets the `wasm_js` backend cfg for the wasm target.

Follow-ups (not in this PR):

- `wrangler dev` smoke test against the built worker.
- Publish via `wrangler deploy` on `omni-remote-cache-worker-v*` tags (needs `CLOUDFLARE_API_TOKEN`).
- Cache `~/.cargo/bin/worker-build` in the setup-tools action.

## Risks

- Workspace feature unification re-enabling Tokio features on wasm.
- `!Send` JS futures vs Axum's `Send` bounds.
- Experimental `worker-build --emscripten` is out of scope.
