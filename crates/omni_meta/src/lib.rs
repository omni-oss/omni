#![allow(clippy::redundant_field_names)]

pub mod error;
pub mod expand;

// @anchor:mods

#[cfg(test)]
mod diagnostic_tests;

pub use error::{Error, ErrorKind};
pub use expand::{
    DEFAULT_META_PROJECTION_DEPTH, EffectiveSource, Materialized, MetaExpand,
    Node, SourceIdentity, expand, first_segment, matches, selected_or_on_path,
};

// @anchor:uses
