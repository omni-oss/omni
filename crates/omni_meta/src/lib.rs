pub mod error;
pub mod expand;

// @anchor:mods

pub use error::{Error, ErrorKind};
pub use expand::{
    DEFAULT_META_PROJECTION_DEPTH, EffectiveSource, Materialized, MetaExpand,
    Node, SourceIdentity, expand, first_segment, matches, selected_or_on_path,
};

// @anchor:uses
