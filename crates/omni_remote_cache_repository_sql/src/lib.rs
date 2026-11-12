//! `sqlx`-backed `QueryExecutor` for native targets. Driver support is
//! selected with cargo features (`sqlite`, `postgres`, `mysql`), all on by
//! default. This crate is empty on wasm32, where D1 is used instead.
#![cfg(not(target_arch = "wasm32"))]

mod executor;

pub use executor::*;
