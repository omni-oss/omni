//! Query-driven repositories and security for `omni_remote_cache_core`.
//!
//! The operator owns the schema and supplies SQL that returns the shape the
//! service needs. This crate has no database driver: it talks to a
//! `QueryExecutor`, implemented by `omni_remote_cache_repository_sql` (native)
//! and `omni_remote_cache_repository_d1` (Cloudflare Workers).

mod executor;
mod hashing;
mod provider;
mod queries;
mod repositories;
mod security;

pub use executor::*;
pub use hashing::*;
pub use provider::*;
pub use queries::*;
pub use repositories::*;
pub use security::*;
