pub mod contributor;
pub mod driver;
pub mod error;
pub mod git;
pub mod lease;
pub mod manager;
pub mod refs;
pub mod registration;
pub mod source;
pub mod sys;

pub use contributor::{InstallOptions, RemoteSourceContributor};
pub use source::{
    LeasedSource, MaterializedSource, RemoteSource, RemoteSourceRef,
};
