#[cfg(feature = "local-disk")]
mod local;

#[cfg(feature = "s3")]
mod s3;

#[cfg(feature = "local-disk")]
pub use local::*;

#[cfg(feature = "s3")]
pub use s3::*;
