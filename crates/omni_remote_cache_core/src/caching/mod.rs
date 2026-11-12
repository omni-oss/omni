//! Opt-in caching decorator over any `DependencyProvider`.
//!
//! Backends never cache. Wrapping a provider caches its repository and
//! security lookups for a bounded time. Errors are never cached.

mod cache;
mod config;
mod provider;

pub use cache::*;
pub use config::*;
pub use provider::*;
