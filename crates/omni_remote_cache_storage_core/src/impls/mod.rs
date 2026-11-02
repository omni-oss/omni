#[cfg(feature = "local-disk")]
mod local;

#[cfg(feature = "in-memory")]
mod memory;

#[cfg(feature = "s3")]
mod s3;

#[cfg(feature = "local-disk")]
pub use local::*;

#[cfg(feature = "s3")]
pub use s3::*;

#[cfg(feature = "in-memory")]
pub use memory::*;
