// Hidden re-export so `bon_builder_mixin!` can reference `$crate::paste`
// without requiring callers to add `paste` to their own dependencies.
#[doc(hidden)]
pub use paste;

mod collect;
pub mod configuration;
pub mod error;
pub mod provider;
pub mod utils;

#[cfg(any(test, feature = "test-utils"))]
pub mod scripted;

pub use collect::*;
pub use configuration::*;
pub use provider::*;

// Re-export the omni_input_schema public surface so consumers of
// omni_input_provider do not need a direct dependency on omni_input_schema.
pub use omni_input_schema::{
    AllowedValue, ArrayBody, BaseInput, BooleanInput, FloatArrayInput,
    FloatInput, Input, InputKind, InputProfile, InputSchema, IntegerArrayInput,
    IntegerInput, ObjectInput, StringArrayInput, StringInput,
    ValidateConfiguration, ValidationConfig, ValidationError, ValidationReport,
    to_json_schema, validate,
};
