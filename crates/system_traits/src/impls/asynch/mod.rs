#[cfg(feature = "real-async-tokio")]
mod real_tokio;

#[cfg(feature = "real-async-tokio")]
pub use real_tokio::*;
