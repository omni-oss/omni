use std::borrow::Cow;

use enumset::EnumSet;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AllowedValue,
    input::{
        FloatArrayInput, InputKind, IntegerArrayInput, StringArrayInput,
        StringInput,
    },
};

/// Trait implemented by marker types that select the extras family for `Input<E>`.
///
/// `E = ()` is the pure-data layer; `E = Generator` (defined in
/// `omni_input_provider`) adds the interactive presentation extras.
///
/// Each slot is named by **input category** (data-side grouping), not by widget,
/// so new presentation families only grow the trait when a genuinely new
/// category is added:
///
/// | Slot      | Used by variants                            |
/// |-----------|---------------------------------------------|
/// | `Base`    | every variant — e.g. `message`, `remember`  |
/// | `Boolean` | `Boolean`                                   |
/// | `String`  | `String`                                    |
/// | `Numeric` | `Integer`, `Float`                          |
/// | `Array`   | `StringArray`, `IntegerArray`, `FloatArray` |
/// | `Object`  | `Object`                                    |
/// | `Option`  | per-`AllowedValue` extras                   |
pub trait InputProfile: Default + Sized {
    /// The set of `InputKind` variants this profile supports.
    ///
    /// The manual `JsonSchema for Input<E>` impl iterates this set so that
    /// unsupported variants are excluded from the emitted schema by construction.
    /// The runtime `validate` pass rejects unsupported variants as a safety net.
    ///
    /// Defaults to `EnumSet::all()` — every variant is supported.
    const SUPPORTED: EnumSet<InputKind> = EnumSet::all();

    type Base: for<'de> Deserialize<'de>
        + Serialize
        + JsonSchema
        + std::fmt::Debug
        + Clone
        + PartialEq
        + Default
        + Send
        + Sync;

    type Boolean: for<'de> Deserialize<'de>
        + Serialize
        + JsonSchema
        + std::fmt::Debug
        + Clone
        + PartialEq
        + Default
        + Send
        + Sync;

    type String: for<'de> Deserialize<'de>
        + Serialize
        + JsonSchema
        + std::fmt::Debug
        + Clone
        + PartialEq
        + Default
        + Send
        + Sync;

    type Numeric: for<'de> Deserialize<'de>
        + Serialize
        + JsonSchema
        + std::fmt::Debug
        + Clone
        + PartialEq
        + Default
        + Send
        + Sync;

    type Array: for<'de> Deserialize<'de>
        + Serialize
        + JsonSchema
        + std::fmt::Debug
        + Clone
        + PartialEq
        + Default
        + Send
        + Sync;

    type Object: for<'de> Deserialize<'de>
        + Serialize
        + JsonSchema
        + std::fmt::Debug
        + Clone
        + PartialEq
        + Default
        + Send
        + Sync;

    type AllowedValueBase: for<'de> Deserialize<'de>
        + Serialize
        + JsonSchema
        + std::fmt::Debug
        + Clone
        + PartialEq
        + Default
        + Send
        + Sync;

    /// Returns `true` when the given base-extras value has `remember` set.
    ///
    /// Used by `validate` to detect the `secret + remember` conflict.
    /// The default returns `false`; the `Generator` marker overrides this
    /// to inspect `GenBase::remember`.
    fn is_remember(_base: &Self::Base) -> bool {
        false
    }

    /// Returns the Tera expression to evaluate as this input's default value,
    /// when no static `default` is present and `use_defaults` is true.
    ///
    /// Applies to all scalar input types (Boolean, String, Integer, Float).
    /// The expression is rendered against the current collect context and the
    /// result is coerced to the input's type.
    ///
    /// The default returns `None`; the `Generator` marker overrides this
    /// to read `GenBase::default_expr`.
    fn dynamic_default_expr(_base_extra: &Self::Base) -> Option<&str> {
        None
    }

    /// Ordered presentation preferences for a string scalar input.
    ///
    /// `collect()` tries each hint in order and uses the first it recognises.
    /// An empty list means "no preference — infer from data":
    /// `secret` → `"password"`, `allowed` → `"select"`, otherwise `"text"`.
    ///
    /// The full input is provided so implementations can factor in data signals
    /// (e.g. `allowed`, `secret`) alongside the profile extras.
    fn string_presentation_hint<'a>(
        _input: &'a StringInput<Self>,
    ) -> Vec<Cow<'a, str>> {
        vec![]
    }

    /// Ordered presentation preferences for a string-array input.
    ///
    /// An empty list means "no preference — infer from data":
    /// `allowed` → `"multi-select"`, otherwise `"free-entry"`.
    fn string_array_presentation_hint<'a>(
        _input: &'a StringArrayInput<Self>,
    ) -> Vec<Cow<'a, str>> {
        vec![]
    }

    /// Ordered presentation preferences for an integer-array input.
    ///
    /// Same inference rules as `string_array_presentation_hint`.
    fn integer_array_presentation_hint<'a>(
        _input: &'a IntegerArrayInput<Self>,
    ) -> Vec<Cow<'a, str>> {
        vec![]
    }

    /// Ordered presentation preferences for a float-array input.
    ///
    /// Same inference rules as `string_array_presentation_hint`.
    fn float_array_presentation_hint<'a>(
        _input: &FloatArrayInput<Self>,
    ) -> Vec<Cow<'static, str>> {
        vec![]
    }

    fn allowed_value_display_name<'a, T>(
        _option: &'a AllowedValue<T, Self>,
    ) -> Option<Cow<'a, str>> {
        None
    }
}

/// The pure-data layer: all extras are `()`.  Every `InputKind` is supported.
impl InputProfile for () {
    type Base = ();
    type Boolean = ();
    type String = ();
    type Numeric = ();
    type Array = ();
    type Object = ();
    type AllowedValueBase = ();
}
