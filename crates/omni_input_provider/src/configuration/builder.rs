//! Fluent builders for [`Input<E>`].
//!
//! Each builder function constructs one variant of `Input<E: InputProfile>`.
//! The shared data-layer fields (`name`, `condition`, `description`, `secret`,
//! `validators`) are accepted by every builder.  Profile-specific extras
//! (`base_extra`, `profile_data`) are optional — they default to
//! `E::Base::default()` / `E::Boolean::default()` etc., which is `()` for the
//! pure-data profile and the appropriate extras struct for `Generator`.
//!
//! Pass `base_extra(GenBase::new("prompt label"))` when building for the
//! Generator profile.
//!
//! ```
//! use omni_input_provider::configuration::builder::boolean;
//!
//! let input = boolean::<()>()
//!     .name("dry_run")
//!     .default(true)
//!     .build();
//! ```

use either::Either;
use omni_input_schema::{
    AllowedValue, ArrayBody, BaseInput, BooleanInput, FloatArrayInput,
    FloatInput, Input, InputProfile, IntegerArrayInput, IntegerInput,
    ObjectInput, StringArrayInput, StringInput, ValidateConfiguration,
};

// ── ValueOrExpr ───────────────────────────────────────────────────────────────

/// A value that is either a concrete literal (`L`) or a Tera expression.
///
/// Used for the `condition` (`if`) builder field: accepts both literal booleans
/// and expression strings, converting to the `Either<L, String>` stored in
/// [`BaseInput::r#if`].
#[derive(Debug, Clone, PartialEq)]
pub enum ValueOrExpr<L> {
    Value(L),
    Expr(String),
}

impl<L> ValueOrExpr<L> {
    #[inline(always)]
    pub fn into_either(self) -> Either<L, String> {
        match self {
            ValueOrExpr::Value(v) => Either::Left(v),
            ValueOrExpr::Expr(e) => Either::Right(e),
        }
    }
}

impl<L> From<ValueOrExpr<L>> for Either<L, String> {
    #[inline(always)]
    fn from(v: ValueOrExpr<L>) -> Self {
        v.into_either()
    }
}

impl<L> From<Either<L, String>> for ValueOrExpr<L> {
    #[inline(always)]
    fn from(e: Either<L, String>) -> Self {
        match e {
            Either::Left(v) => ValueOrExpr::Value(v),
            Either::Right(s) => ValueOrExpr::Expr(s),
        }
    }
}

// Strings are always treated as Tera expressions (never overlap with concrete
// bool/i64/f64 variants below).
impl<L> From<String> for ValueOrExpr<L> {
    #[inline(always)]
    fn from(s: String) -> Self {
        ValueOrExpr::Expr(s)
    }
}

impl<L> From<&str> for ValueOrExpr<L> {
    #[inline(always)]
    fn from(s: &str) -> Self {
        ValueOrExpr::Expr(s.to_string())
    }
}

impl From<bool> for ValueOrExpr<bool> {
    #[inline(always)]
    fn from(v: bool) -> Self {
        ValueOrExpr::Value(v)
    }
}

impl From<f64> for ValueOrExpr<f64> {
    #[inline(always)]
    fn from(v: f64) -> Self {
        ValueOrExpr::Value(v)
    }
}

impl From<i64> for ValueOrExpr<i64> {
    #[inline(always)]
    fn from(v: i64) -> Self {
        ValueOrExpr::Value(v)
    }
}

impl From<i32> for ValueOrExpr<i64> {
    #[inline(always)]
    fn from(v: i32) -> Self {
        ValueOrExpr::Value(v as i64)
    }
}

// ── build_base ────────────────────────────────────────────────────────────────

fn build_base(
    name: String,
    condition: Option<ValueOrExpr<bool>>,
    description: Option<String>,
    secret: bool,
    validators: Vec<ValidateConfiguration>,
) -> BaseInput {
    BaseInput {
        name,
        r#if: condition.map(ValueOrExpr::into_either),
        validators,
        secret,
        description,
    }
}

// ── ValidateConfiguration conversions ────────────────────────────────────────
// From<&str>, From<String>, From<(&str,&str)> live in omni_input_schema::base
// (orphan rule). Re-exported here via the omni_input_schema re-export in lib.rs.
// The `validator()` builder below offers the fluent alternative.

/// Build a [`ValidateConfiguration`] with an optional error message.
///
/// `condition` accepts a literal `bool` or a Tera expression string.
#[::bon::builder(finish_fn = build)]
pub fn validator(
    #[builder(into)] condition: ValueOrExpr<bool>,
    #[builder(into)] error_message: Option<String>,
) -> ValidateConfiguration {
    ValidateConfiguration {
        condition: condition.into_either(),
        error_message,
    }
}

// ── AllowedValue conversions ──────────────────────────────────────────────────
// From<&str>, From<String>, From<(&str,&str)> live in omni_input_schema::allowed.
// The `allowed_value()` builder below offers the fluent alternative.

/// Build an [`AllowedValue<String, OptionExtras>`] entry for select / multi-select inputs.
///
/// `value` defaults to `name` when omitted.
#[::bon::builder(finish_fn = build)]
pub fn allowed_value<E: InputProfile>(
    #[builder(into)] value: String,
    #[builder(into)] description: Option<String>,
    #[builder(into, default)] base_extra: E::AllowedValueBase,
) -> AllowedValue<String, E> {
    AllowedValue {
        value,
        description,
        base_extra,
    }
}

// ── Input builders ────────────────────────────────────────────────────────────

/// Build a `Boolean` input (`Input<E>::Boolean`).
#[::bon::builder(finish_fn = build)]
pub fn boolean<E: InputProfile>(
    #[builder(into)] name: String,
    #[builder(into)] condition: Option<ValueOrExpr<bool>>,
    #[builder(into)] description: Option<String>,
    #[builder(default)] secret: bool,
    #[builder(default, with = |v: impl IntoIterator<Item = impl Into<ValidateConfiguration>>|
        v.into_iter().map(Into::into).collect())]
    validators: Vec<ValidateConfiguration>,
    #[builder(into)] default: Option<bool>,
    #[builder(default)] base_extra: E::Base,
    #[builder(default)] profile_data: E::Boolean,
) -> Input<E> {
    Input::Boolean(BooleanInput {
        base: build_base(name, condition, description, secret, validators),
        default,
        base_extra,
        profile_data,
    })
}

/// Ergonomic alias for `boolean` — maps naturally to a yes/no prompt.
#[::bon::builder(finish_fn = build)]
pub fn confirm<E: InputProfile>(
    #[builder(into)] name: String,
    #[builder(into)] condition: Option<ValueOrExpr<bool>>,
    #[builder(into)] description: Option<String>,
    #[builder(default)] secret: bool,
    #[builder(default, with = |v: impl IntoIterator<Item = impl Into<ValidateConfiguration>>|
        v.into_iter().map(Into::into).collect())]
    validators: Vec<ValidateConfiguration>,
    #[builder(into)] default: Option<bool>,
    #[builder(default)] base_extra: E::Base,
    #[builder(default)] profile_data: E::Boolean,
) -> Input<E> {
    Input::Boolean(BooleanInput {
        base: build_base(name, condition, description, secret, validators),
        default,
        base_extra,
        profile_data,
    })
}

/// Build a `String` input (`Input<E>::String`).
#[::bon::builder(finish_fn = build)]
pub fn text<E: InputProfile>(
    #[builder(into)] name: String,
    #[builder(into)] condition: Option<ValueOrExpr<bool>>,
    #[builder(into)] description: Option<String>,
    #[builder(default)] secret: bool,
    #[builder(default, with = |v: impl IntoIterator<Item = impl Into<ValidateConfiguration>>|
        v.into_iter().map(Into::into).collect())]
    validators: Vec<ValidateConfiguration>,
    #[builder(with = |opts: impl IntoIterator<Item = impl Into<AllowedValue<String, E>>>|
        opts.into_iter().map(Into::into).collect())]
    allowed: Option<Vec<AllowedValue<String, E>>>,
    #[builder(into)] default: Option<String>,
    #[builder(default)] base_extra: E::Base,
    #[builder(default)] profile_data: E::String,
) -> Input<E> {
    Input::String(StringInput {
        base: build_base(name, condition, description, secret, validators),
        allowed,
        default,
        base_extra,
        profile_data,
    })
}

/// Build a `String` input with `secret: true` by default (password widget inferred).
#[::bon::builder(finish_fn = build)]
pub fn password<E: InputProfile>(
    #[builder(into)] name: String,
    #[builder(into)] condition: Option<ValueOrExpr<bool>>,
    #[builder(into)] description: Option<String>,
    #[builder(default = true)] secret: bool,
    #[builder(default, with = |v: impl IntoIterator<Item = impl Into<ValidateConfiguration>>|
        v.into_iter().map(Into::into).collect())]
    validators: Vec<ValidateConfiguration>,
    #[builder(into)] default: Option<String>,
    #[builder(default)] base_extra: E::Base,
    #[builder(default)] profile_data: E::String,
) -> Input<E> {
    Input::String(StringInput {
        base: build_base(name, condition, description, secret, validators),
        allowed: None,
        default,
        base_extra,
        profile_data,
    })
}

/// Build a `String` input with a required `allowed` list (select widget inferred).
#[::bon::builder(finish_fn = build)]
pub fn select<E: InputProfile>(
    #[builder(into)] name: String,
    #[builder(into)] condition: Option<ValueOrExpr<bool>>,
    #[builder(into)] description: Option<String>,
    #[builder(default)] secret: bool,
    #[builder(default, with = |v: impl IntoIterator<Item = impl Into<ValidateConfiguration>>|
        v.into_iter().map(Into::into).collect())]
    validators: Vec<ValidateConfiguration>,
    #[builder(with = |opts: impl IntoIterator<Item = impl Into<AllowedValue<String, E>>>|
        opts.into_iter().map(Into::into).collect())]
    allowed: Vec<AllowedValue<String, E>>,
    #[builder(into)] default: Option<String>,
    #[builder(default)] base_extra: E::Base,
    #[builder(default)] profile_data: E::String,
) -> Input<E> {
    Input::String(StringInput {
        base: build_base(name, condition, description, secret, validators),
        allowed: Some(allowed),
        default,
        base_extra,
        profile_data,
    })
}

/// Build a `StringArray` input (`Input<E>::StringArray`).
#[::bon::builder(finish_fn = build)]
pub fn multi_select<E: InputProfile>(
    #[builder(into)] name: String,
    #[builder(into)] condition: Option<ValueOrExpr<bool>>,
    #[builder(into)] description: Option<String>,
    #[builder(default)] secret: bool,
    #[builder(default, with = |v: impl IntoIterator<Item = impl Into<ValidateConfiguration>>|
        v.into_iter().map(Into::into).collect())]
    validators: Vec<ValidateConfiguration>,
    #[builder(with = |opts: impl IntoIterator<Item = impl Into<AllowedValue<String, E>>>|
        opts.into_iter().map(Into::into).collect())]
    allowed: Option<Vec<AllowedValue<String, E>>>,
    #[builder(with = |defaults: impl IntoIterator<Item = impl Into<String>>|
        defaults.into_iter().map(Into::into).collect())]
    default: Option<Vec<String>>,
    #[builder(default)] base_extra: E::Base,
    #[builder(default)] profile_data: E::Array,
) -> Input<E> {
    Input::StringArray(StringArrayInput {
        base: build_base(name, condition, description, secret, validators),
        body: ArrayBody { allowed, default },
        base_extra,
        profile_data,
    })
}

/// Build an `Integer` input (`Input<E>::Integer`).
#[::bon::builder(finish_fn = build)]
pub fn integer<E: InputProfile>(
    #[builder(into)] name: String,
    #[builder(into)] condition: Option<ValueOrExpr<bool>>,
    #[builder(into)] description: Option<String>,
    #[builder(default)] secret: bool,
    #[builder(default, with = |v: impl IntoIterator<Item = impl Into<ValidateConfiguration>>|
        v.into_iter().map(Into::into).collect())]
    validators: Vec<ValidateConfiguration>,
    #[builder(with = |opts: impl IntoIterator<Item = impl Into<AllowedValue<i64, E>>>|
        opts.into_iter().map(Into::into).collect())]
    allowed: Option<Vec<AllowedValue<i64, E>>>,
    #[builder(into)] default: Option<i64>,
    #[builder(default)] base_extra: E::Base,
    #[builder(default)] profile_data: E::Numeric,
) -> Input<E> {
    Input::Integer(IntegerInput {
        base: build_base(name, condition, description, secret, validators),
        allowed,
        default,
        base_extra,
        profile_data,
    })
}

/// Build an `IntegerArray` input (`Input<E>::IntegerArray`).
#[::bon::builder(finish_fn = build)]
pub fn integer_array<E: InputProfile>(
    #[builder(into)] name: String,
    #[builder(into)] condition: Option<ValueOrExpr<bool>>,
    #[builder(into)] description: Option<String>,
    #[builder(default)] secret: bool,
    #[builder(default, with = |v: impl IntoIterator<Item = impl Into<ValidateConfiguration>>|
        v.into_iter().map(Into::into).collect())]
    validators: Vec<ValidateConfiguration>,
    #[builder(with = |opts: impl IntoIterator<Item = impl Into<AllowedValue<i64, E>>>|
        opts.into_iter().map(Into::into).collect())]
    allowed: Option<Vec<AllowedValue<i64, E>>>,
    #[builder(into)] default: Option<Vec<i64>>,
    #[builder(default)] base_extra: E::Base,
    #[builder(default)] profile_data: E::Array,
) -> Input<E> {
    Input::IntegerArray(IntegerArrayInput {
        base: build_base(name, condition, description, secret, validators),
        base_extra,
        profile_data,
        body: ArrayBody { allowed, default },
    })
}

/// Build a `Float` input (`Input<E>::Float`).
#[::bon::builder(finish_fn = build)]
pub fn float<E: InputProfile>(
    #[builder(into)] name: String,
    #[builder(into)] condition: Option<ValueOrExpr<bool>>,
    #[builder(into)] description: Option<String>,
    #[builder(default)] secret: bool,
    #[builder(default, with = |v: impl IntoIterator<Item = impl Into<ValidateConfiguration>>|
        v.into_iter().map(Into::into).collect())]
    validators: Vec<ValidateConfiguration>,
    #[builder(with = |opts: impl IntoIterator<Item = impl Into<AllowedValue<f64, E>>>|
        opts.into_iter().map(Into::into).collect())]
    allowed: Option<Vec<AllowedValue<f64, E>>>,
    #[builder(into)] default: Option<f64>,
    #[builder(default)] base_extra: E::Base,
    #[builder(default)] profile_data: E::Numeric,
) -> Input<E> {
    Input::Float(FloatInput {
        base: build_base(name, condition, description, secret, validators),
        allowed,
        default,
        base_extra,
        profile_data,
    })
}

/// Build a `FloatArray` input (`Input<E>::FloatArray`).
#[::bon::builder(finish_fn = build)]
pub fn float_array<E: InputProfile>(
    #[builder(into)] name: String,
    #[builder(into)] condition: Option<ValueOrExpr<bool>>,
    #[builder(into)] description: Option<String>,
    #[builder(default)] secret: bool,
    #[builder(default, with = |v: impl IntoIterator<Item = impl Into<ValidateConfiguration>>|
        v.into_iter().map(Into::into).collect())]
    validators: Vec<ValidateConfiguration>,
    #[builder(with = |opts: impl IntoIterator<Item = impl Into<AllowedValue<f64, E>>>|
        opts.into_iter().map(Into::into).collect())]
    allowed: Option<Vec<AllowedValue<f64, E>>>,
    #[builder(into)] default: Option<Vec<f64>>,
    #[builder(default)] base_extra: E::Base,
    #[builder(default)] profile_data: E::Array,
) -> Input<E> {
    Input::FloatArray(FloatArrayInput {
        base: build_base(name, condition, description, secret, validators),
        base_extra,
        profile_data,
        body: ArrayBody { allowed, default },
    })
}

/// Build an `Object` input (`Input<E>::Object`).
///
/// `fields` accepts any iterator of `Input<E>` and defaults to an empty list.
#[::bon::builder(finish_fn = build)]
pub fn object<E: InputProfile>(
    #[builder(into)] name: String,
    #[builder(into)] condition: Option<ValueOrExpr<bool>>,
    #[builder(into)] description: Option<String>,
    #[builder(default)] secret: bool,
    #[builder(default, with = |v: impl IntoIterator<Item = impl Into<ValidateConfiguration>>|
        v.into_iter().map(Into::into).collect())]
    validators: Vec<ValidateConfiguration>,
    #[builder(default, with = |v: impl IntoIterator<Item = Input<E>>|
        v.into_iter().collect())]
    fields: Vec<Input<E>>,
    #[builder(default)] base_extra: E::Base,
    #[builder(default)] profile_data: E::Object,
) -> Input<E> {
    Input::Object(ObjectInput {
        base: build_base(name, condition, description, secret, validators),
        fields,
        base_extra,
        profile_data,
    })
}

#[cfg(test)]
mod tests {
    use either::Either;

    use super::*;
    use omni_input_schema::{Input, InputKind};

    // ── boolean / confirm ──────────────────────────────────────────────────

    #[test]
    fn boolean_sets_name_and_default() {
        let input = boolean::<()>().name("dry_run").default(true).build();

        assert_eq!(input.base().name, "dry_run");
        assert_eq!(input.kind(), InputKind::Boolean);

        let Input::Boolean(b) = input else {
            panic!("expected Boolean")
        };
        assert_eq!(b.default, Some(true));
    }

    #[test]
    fn confirm_is_an_alias_for_boolean() {
        let input = confirm::<()>().name("ok").build();
        assert_eq!(input.kind(), InputKind::Boolean);
    }

    #[test]
    fn condition_string_is_treated_as_expression() {
        let input = text::<()>()
            .name("extra")
            .condition("{{ inputs.mode == 'advanced' }}")
            .build();

        assert_eq!(
            input.base().r#if,
            Some(Either::Right("{{ inputs.mode == 'advanced' }}".to_string()))
        );
    }

    // ── text / string ──────────────────────────────────────────────────────

    #[test]
    fn text_sets_string_default() {
        let input = text::<()>().name("greeting").default("hello").build();

        assert_eq!(input.kind(), InputKind::String);
        let Input::String(s) = input else { panic!() };
        assert_eq!(s.default.as_deref(), Some("hello"));
    }

    // ── password ──────────────────────────────────────────────────────────

    #[test]
    fn password_defaults_secret_to_true() {
        let input = password::<()>().name("tok").build();
        assert!(input.base().secret, "password inputs are secret by default");
    }

    // ── select ────────────────────────────────────────────────────────────

    #[test]
    fn select_options_accept_strings_tuples_and_builders() {
        let input = select::<()>()
            .name("color")
            .allowed([
                AllowedValue::<String, ()>::from("red"),
                AllowedValue::<String, ()>::from("green"),
            ])
            .default("red")
            .build();

        assert_eq!(input.kind(), InputKind::String);
        let Input::String(s) = input else { panic!() };
        assert_eq!(s.allowed.as_ref().unwrap().len(), 2);
        assert_eq!(s.allowed.as_ref().unwrap()[0].value, "red");
        assert_eq!(s.default.as_deref(), Some("red"));
    }

    // ── multi_select ──────────────────────────────────────────────────────

    #[test]
    fn multi_select_default_accepts_an_iterator_of_strings() {
        let input = multi_select::<()>()
            .name("langs")
            .allowed(["rust", "go", "zig"])
            .default(["rust", "go"])
            .build();

        assert_eq!(input.kind(), InputKind::StringArray);
        let Input::StringArray(sa) = input else {
            panic!()
        };
        assert_eq!(sa.body.allowed.as_ref().unwrap().len(), 3);
        assert_eq!(
            sa.body.default,
            Some(vec!["rust".to_string(), "go".to_string()])
        );
    }

    // ── integer / float ───────────────────────────────────────────────────

    #[test]
    fn integer_sets_typed_default() {
        let input = integer::<()>().name("count").default(42_i64).build();
        assert_eq!(input.kind(), InputKind::Integer);
        let Input::Integer(i) = input else { panic!() };
        assert_eq!(i.default, Some(42));
    }

    #[test]
    fn float_sets_typed_default() {
        let input = float::<()>().name("rate").default(1.5_f64).build();
        assert_eq!(input.kind(), InputKind::Float);
        let Input::Float(f) = input else { panic!() };
        assert!((f.default.unwrap() - 1.5).abs() < 1e-10);
    }

    // ── integer_array ─────────────────────────────────────────────────────

    #[test]
    fn integer_array_sets_name_and_default() {
        let input = integer_array::<()>()
            .name("counts")
            .default(vec![1_i64, 2, 3])
            .build();

        assert_eq!(input.kind(), InputKind::IntegerArray);
        let Input::IntegerArray(ia) = input else {
            panic!()
        };
        assert_eq!(ia.base.name, "counts");
        assert_eq!(ia.body.default, Some(vec![1, 2, 3]));
    }

    #[test]
    fn integer_array_allowed_constrains_valid_values() {
        let input = integer_array::<()>()
            .name("ports")
            .allowed([
                AllowedValue {
                    value: 80_i64,
                    description: None,
                    base_extra: (),
                },
                AllowedValue {
                    value: 443_i64,
                    description: None,
                    base_extra: (),
                },
            ])
            .build();

        let Input::IntegerArray(ia) = input else {
            panic!()
        };
        let allowed = ia.body.allowed.unwrap();
        assert_eq!(allowed.len(), 2);
        assert_eq!(allowed[0].value, 80);
        assert_eq!(allowed[1].value, 443);
    }

    // ── float_array ───────────────────────────────────────────────────────

    #[test]
    fn float_array_sets_name_and_default() {
        let input = float_array::<()>()
            .name("coeffs")
            .default(vec![1.0_f64, 0.5])
            .build();

        assert_eq!(input.kind(), InputKind::FloatArray);
        let Input::FloatArray(fa) = input else {
            panic!()
        };
        assert_eq!(fa.base.name, "coeffs");
        let default = fa.body.default.unwrap();
        assert!((default[0] - 1.0).abs() < 1e-10);
        assert!((default[1] - 0.5).abs() < 1e-10);
    }

    #[test]
    fn float_array_allowed_constrains_valid_values() {
        let input = float_array::<()>()
            .name("scale")
            .allowed([
                AllowedValue {
                    value: 0.5_f64,
                    description: None,
                    base_extra: (),
                },
                AllowedValue {
                    value: 1.0_f64,
                    description: None,
                    base_extra: (),
                },
                AllowedValue {
                    value: 2.0_f64,
                    description: None,
                    base_extra: (),
                },
            ])
            .build();

        let Input::FloatArray(fa) = input else {
            panic!()
        };
        assert_eq!(fa.body.allowed.unwrap().len(), 3);
    }

    // ── object ────────────────────────────────────────────────────────────

    #[test]
    fn object_sets_name_and_fields() {
        let input = object::<()>()
            .name("author")
            .fields([
                text::<()>().name("first_name").build(),
                text::<()>().name("last_name").build(),
            ])
            .build();

        assert_eq!(input.kind(), InputKind::Object);
        let Input::Object(obj) = input else { panic!() };
        assert_eq!(obj.base.name, "author");
        assert_eq!(obj.fields.len(), 2);
        assert_eq!(obj.fields[0].base().name, "first_name");
        assert_eq!(obj.fields[1].base().name, "last_name");
    }

    #[test]
    fn object_with_no_fields_defaults_to_empty() {
        let input = object::<()>().name("meta").build();
        let Input::Object(obj) = input else { panic!() };
        assert!(obj.fields.is_empty());
    }

    // ── validators ────────────────────────────────────────────────────────

    #[test]
    fn validators_are_collected_from_various_conversions() {
        let input = text::<()>()
            .name("name")
            .validators([
                ValidateConfiguration::from("{{ value | length > 0 }}"),
                ValidateConfiguration::from((
                    "{{ value | length < 20 }}",
                    "too long",
                )),
                validator()
                    .condition("{{ value != 'admin' }}")
                    .error_message("reserved")
                    .build(),
            ])
            .build();

        let Input::String(s) = input else { panic!() };
        assert_eq!(s.base.validators.len(), 3);
        assert_eq!(
            s.base.validators[1].error_message.as_deref(),
            Some("too long")
        );
    }

    // ── allowed_value builder ─────────────────────────────────────────────

    #[test]
    fn allowed_value_defaults_value_to_name() {
        let av = allowed_value::<()>().value("Yes").build();
        assert_eq!(av.value, "Yes");
    }

    #[test]
    fn allowed_value_accepts_explicit_value() {
        let av = allowed_value::<()>()
            .value("mit")
            .description("MIT License")
            .build();
        assert_eq!(av.value, "mit");
        assert_eq!(av.description.as_deref(), Some("MIT License"));
    }
}
