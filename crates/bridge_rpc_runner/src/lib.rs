#![feature(decl_macro)]

//! # `bridge_rpc_runner`
//!
//! Spawns a JavaScript runtime running the vendored `@omni-oss/bridge-service`
//! bundle and connects a bidirectional bridge RPC to it over stdio.
//!
//! This crate is the reusable process-spawning seam shared by every subsystem
//! that drives JS/TS scripts through the bridge (generators today, tools
//! later). It owns three concerns and nothing subsystem-specific:
//!
//! * [`DelegatingJsRuntimeOption`] — runtime selection / `PATH` detection.
//! * [`VendoredBridgeService`] — materializing the embedded bundle on disk.
//! * [`BridgeServiceRunner`] — launching the runtime under a capability
//!   [`SpawnPolicy`](omni_capability_enforcement::SpawnPolicy) and forwarding
//!   RPC requests.
//! * [`RunnerPool`] — a lazily-spawned, keyed cache of runners; a pure
//!   mechanism that leaves all policy/enforcement decisions to a caller-
//!   supplied factory closure.

mod error;
mod pool;
mod runner;
mod runtime;
mod vendor;

pub use error::*;
pub use pool::*;
pub use runner::*;
pub use runtime::*;
pub use vendor::*;
