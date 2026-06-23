//! Generic, presentation-free input model for omni (RFC 0003).
//!
//! # Overview
//!
//! This crate provides a single generic input type `Input<E: InputProfile>`
//! whose variants are **data types** (`Boolean`, `String`, `Integer`, `Float`,
//! `StringArray`, `IntegerArray`, `FloatArray`, `Object`).  Presentation extras
//! (messages, widget hints, option labels) are layered on top by the marker type
//! `E` through the [`InputProfile`] trait.
//!
//! | Marker `E`        | Where defined         | Extras                          |
//! |-------------------|-----------------------|---------------------------------|
//! | `()`              | this crate            | none — pure data                |
//! | `Generator`       | `omni_generator_configurations` | message, widget hints, remember |
//!
//! # Key types
//!
//! - [`Input<E>`] — the generic input enum.
//! - [`InputSchema`] — type alias for `Input<()>` used by tools, plugins, MCP.
//! - [`InputProfile`] — marker trait selecting the extras family.
//! - [`InputKind`] — fieldless discriminant; `EnumSetType` so `SUPPORTED` is const.
//! - [`BaseInput`] — data fields every consumer reads: `name`, `if`, `validators`,
//!   `secret`, `description`.
//! - [`AllowedValue<T, TOpt>`] — typed allowed-value with optional extras.
//! - [`ArrayBody<T, TOpt>`] — `allowed` + `default` for array variants.
//! - [`OptionExtras`] — presentation extras for `AllowedValue` entries.
//! - [`validate`] — validate pre-supplied values against an input list.
//! - [`to_json_schema`] — project an input list to a JSON Schema values object.

pub mod allowed;
pub mod base;
pub mod error;
pub mod input;
mod json_schema;
mod parsers;
pub mod profile;
pub mod validate;

pub use allowed::{AllowedValue, ArrayBody};
pub use base::{BaseInput, ValidateConfiguration};
pub use error::{Error, ErrorKind};
pub use input::{
    BooleanInput, FloatArrayInput, FloatInput, Input, InputKind, InputSchema,
    IntegerArrayInput, IntegerInput, ObjectInput, StringArrayInput,
    StringInput,
};
pub use json_schema::to_json_schema;
pub use profile::InputProfile;
pub use validate::{
    ValidationConfig, ValidationError, ValidationReport, validate,
    validate_boolean_expression_result, validate_value,
};
