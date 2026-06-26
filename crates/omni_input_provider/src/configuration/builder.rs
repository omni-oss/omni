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

use maps::UnorderedMap;
use omni_config_types::MaybeExpr;
use omni_input_schema::{
    AllowedValue, ArrayBody, BaseInput, BooleanInput, FloatArrayInput,
    FloatInput, Input, InputProfile, IntegerArrayInput, IntegerInput,
    ObjectInput, StringArrayInput, StringInput, ValidateConfiguration,
    input::InputValue,
};

// ── ValueOrExpr ───────────────────────────────────────────────────────────────

/// A value that is either a concrete literal (`L`) or a Tera expression.
///
/// Used for the `condition` (`if`) builder field: accepts both literal booleans
/// and expression strings, converting to the `Either<L, String>` stored in
/// [`BaseInput::r#if`].

// ── build_base ────────────────────────────────────────────────────────────────

fn build_base(
    name: String,
    condition: Option<MaybeExpr<bool>>,
    description: Option<String>,
    secret: bool,
    validators: Vec<ValidateConfiguration>,
) -> BaseInput {
    BaseInput {
        name,
        r#if: condition,
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
#[::bon::builder(finish_fn = build, state_mod(vis = "pub"))]
pub fn validator(
    #[builder(into)] condition: MaybeExpr<bool>,
    #[builder(into)] error_message: Option<String>,
) -> ValidateConfiguration {
    ValidateConfiguration {
        condition,
        error_message,
    }
}

// ── AllowedValue conversions ──────────────────────────────────────────────────
// From<&str>, From<String>, From<(&str,&str)> live in omni_input_schema::allowed.
// The `allowed_value()` builder below offers the fluent alternative.

/// Build an [`AllowedValue<String, OptionExtras>`] entry for select / multi-select inputs.
///
/// `value` defaults to `name` when omitted.
#[::bon::builder(finish_fn = build, state_mod(vis = "pub"))]
pub fn allowed<E: InputProfile>(
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
#[::bon::builder(finish_fn = build, state_mod(vis = "pub"))]
pub fn boolean<E: InputProfile>(
    #[builder(into)] name: String,
    #[builder(into)] condition: Option<MaybeExpr<bool>>,
    #[builder(into)] description: Option<String>,
    #[builder(default)] secret: bool,
    #[builder(default, with = |v: impl IntoIterator<Item = impl Into<ValidateConfiguration>>|
        v.into_iter().map(Into::into).collect())]
    validators: Vec<ValidateConfiguration>,
    #[builder(into)] default: Option<MaybeExpr<bool>>,
    #[builder(default)] base_extra: E::Base,
    #[builder(default)] boolean_extra: E::Boolean,
) -> Input<E> {
    Input::Boolean(BooleanInput {
        base: build_base(name, condition, description, secret, validators),
        default,
        base_extra,
        boolean_extra,
    })
}

/// Build a `String` input (`Input<E>::String`).
#[::bon::builder(finish_fn = build, state_mod(vis = "pub"))]
pub fn string<E: InputProfile>(
    #[builder(into)] name: String,
    #[builder(into)] condition: Option<MaybeExpr<bool>>,
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
    #[builder(default)] string_extra: E::String,
) -> Input<E> {
    Input::String(StringInput {
        base: build_base(name, condition, description, secret, validators),
        allowed,
        default,
        base_extra,
        string_extra,
    })
}

/// Build a `String` input with `secret: true` by default (password widget inferred).
#[::bon::builder(finish_fn = build, state_mod(vis = "pub"))]
pub fn password<E: InputProfile>(
    #[builder(into)] name: String,
    #[builder(into)] condition: Option<MaybeExpr<bool>>,
    #[builder(into)] description: Option<String>,
    #[builder(default = true)] secret: bool,
    #[builder(default, with = |v: impl IntoIterator<Item = impl Into<ValidateConfiguration>>|
        v.into_iter().map(Into::into).collect())]
    validators: Vec<ValidateConfiguration>,
    #[builder(into)] default: Option<String>,
    #[builder(default)] base_extra: E::Base,
    #[builder(default)] string_extra: E::String,
) -> Input<E> {
    Input::String(StringInput {
        base: build_base(name, condition, description, secret, validators),
        allowed: None,
        default,
        base_extra,
        string_extra,
    })
}

/// Build a `StringArray` input (`Input<E>::StringArray`).
#[::bon::builder(finish_fn = build, state_mod(vis = "pub"))]
pub fn string_array<E: InputProfile>(
    #[builder(into)] name: String,
    #[builder(into)] condition: Option<MaybeExpr<bool>>,
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
    #[builder(default)] array_extra: E::Array,
) -> Input<E> {
    Input::StringArray(StringArrayInput {
        base: build_base(name, condition, description, secret, validators),
        body: ArrayBody { allowed },
        default,
        base_extra,
        array_extra,
    })
}

/// Build an `Integer` input (`Input<E>::Integer`).
#[::bon::builder(finish_fn = build, state_mod(vis = "pub"))]
pub fn integer<E: InputProfile>(
    #[builder(into)] name: String,
    #[builder(into)] condition: Option<MaybeExpr<bool>>,
    #[builder(into)] description: Option<String>,
    #[builder(default)] secret: bool,
    #[builder(default, with = |v: impl IntoIterator<Item = impl Into<ValidateConfiguration>>|
        v.into_iter().map(Into::into).collect())]
    validators: Vec<ValidateConfiguration>,
    #[builder(with = |opts: impl IntoIterator<Item = impl Into<AllowedValue<i64, E>>>|
        opts.into_iter().map(Into::into).collect())]
    allowed: Option<Vec<AllowedValue<i64, E>>>,
    #[builder(into)] default: Option<MaybeExpr<i64>>,
    #[builder(default)] base_extra: E::Base,
    #[builder(default)] numeric_extra: E::Numeric,
) -> Input<E> {
    Input::Integer(IntegerInput {
        base: build_base(name, condition, description, secret, validators),
        allowed,
        default,
        base_extra,
        numeric_extra,
    })
}

/// Build an `IntegerArray` input (`Input<E>::IntegerArray`).
#[::bon::builder(finish_fn = build, state_mod(vis = "pub"))]
pub fn integer_array<E: InputProfile>(
    #[builder(into)] name: String,
    #[builder(into)] condition: Option<MaybeExpr<bool>>,
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
    #[builder(default)] array_extra: E::Array,
) -> Input<E> {
    Input::IntegerArray(IntegerArrayInput {
        base: build_base(name, condition, description, secret, validators),
        base_extra,
        array_extra,
        body: ArrayBody { allowed },
        default,
    })
}

/// Build a `Float` input (`Input<E>::Float`).
#[::bon::builder(finish_fn = build, state_mod(vis = "pub"))]
pub fn float<E: InputProfile>(
    #[builder(into)] name: String,
    #[builder(into)] condition: Option<MaybeExpr<bool>>,
    #[builder(into)] description: Option<String>,
    #[builder(default)] secret: bool,
    #[builder(default, with = |v: impl IntoIterator<Item = impl Into<ValidateConfiguration>>|
        v.into_iter().map(Into::into).collect())]
    validators: Vec<ValidateConfiguration>,
    #[builder(with = |opts: impl IntoIterator<Item = impl Into<AllowedValue<f64, E>>>|
        opts.into_iter().map(Into::into).collect())]
    allowed: Option<Vec<AllowedValue<f64, E>>>,
    #[builder(into)] default: Option<MaybeExpr<f64>>,
    #[builder(default)] base_extra: E::Base,
    #[builder(default)] numeric_extra: E::Numeric,
) -> Input<E> {
    Input::Float(FloatInput {
        base: build_base(name, condition, description, secret, validators),
        allowed,
        default,
        base_extra,
        numeric_extra,
    })
}

/// Build a `FloatArray` input (`Input<E>::FloatArray`).
#[::bon::builder(finish_fn = build, state_mod(vis = "pub"))]
pub fn float_array<E: InputProfile>(
    #[builder(into)] name: String,
    #[builder(into)] condition: Option<MaybeExpr<bool>>,
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
    #[builder(default)] array_extra: E::Array,
) -> Input<E> {
    Input::FloatArray(FloatArrayInput {
        base: build_base(name, condition, description, secret, validators),
        base_extra,
        array_extra,
        default,
        body: ArrayBody { allowed },
    })
}

/// Build an `Object` input (`Input<E>::Object`).
///
/// `fields` accepts any iterator of `Input<E>` and defaults to an empty list.
#[::bon::builder(finish_fn = build, state_mod(vis = "pub"))]
pub fn object<E: InputProfile>(
    #[builder(into)] name: String,
    #[builder(into)] condition: Option<MaybeExpr<bool>>,
    #[builder(into)] description: Option<String>,
    #[builder(default)] secret: bool,
    #[builder(default, with = |v: impl IntoIterator<Item = impl Into<ValidateConfiguration>>|
        v.into_iter().map(Into::into).collect())]
    validators: Vec<ValidateConfiguration>,
    #[builder(default, with = |v: impl IntoIterator<Item = Input<E>>|
        v.into_iter().collect())]
    fields: Vec<Input<E>>,
    #[builder(into)] default: Option<UnorderedMap<String, InputValue>>,
    #[builder(default)] base_extra: E::Base,
    #[builder(default)] object_extra: E::Object,
) -> Input<E> {
    Input::Object(ObjectInput {
        base: build_base(name, condition, description, secret, validators),
        fields,
        base_extra,
        object_extra,
        default,
    })
}

#[macro_export]
macro_rules! bon_builder_extend {
    // ── Minimal form ─────────────────────────────────────────────────────────
    // Infers everything, including the extension trait name, from four inputs.
    // Trait name convention: base.builder + PascalCase(mixin.ctor) + "Ext"
    //   e.g. StringBuilder + gen_base → StringBuilderGenBaseExt
    //
    // Use the short form below to supply a custom trait name.
    (
        profile = $profile:ident,
        base = {
            builder    = $base_ty:ident,
            set_method = $base_set_method:ident $(,)?
        },
        mixin = {
            ctor = $mixin_ctor:ident $(,)?
        } $(,)?
    ) => {
        $crate::paste::paste! {
            $crate::bon_builder_extend!(
                profile = $profile,
                base = {
                    builder    = $base_ty,
                    set_method = $base_set_method,
                },
                mixin = {
                    ctor = $mixin_ctor,
                },
                ext = {
                    trait = [<$base_ty $mixin_ctor:camel Ext>],
                },
            );
        }
    };

    // ── Short form ───────────────────────────────────────────────────────────
    // Infers the seven boilerplate identifiers from five inputs
    // using bon's naming conventions:
    //
    //   base.state_mod       = snake_case(base.builder)
    //   base.state_field     = PascalCase(base.set_method)
    //   base.state_set_field = "Set" + PascalCase(base.set_method)
    //   mixin.builder        = PascalCase(mixin.ctor) + "Builder"
    //   mixin.state_mod      = mixin.ctor + "_builder"
    //   mixin.finish_fn      = build   (bon's default finish fn name)
    //   ext.method           = mixin.ctor
    //
    // Use the explicit form below when any convention is broken.
    (
        profile = $profile:ident,
        base = {
            builder    = $base_ty:ident,
            set_method = $base_set_method:ident $(,)?
        },
        mixin = {
            ctor = $mixin_ctor:ident $(,)?
        },
        ext = {
            trait = $ext_trait_ty:ident $(,)?
        } $(,)?
    ) => {
        $crate::paste::paste! {
            $crate::bon_builder_extend!(
                profile = $profile,
                base = {
                    builder         = $base_ty,
                    state_mod       = [<$base_ty:snake>],
                    set_method      = $base_set_method,
                    state_field     = [<$base_set_method:camel>],
                    state_set_field = [<Set $base_set_method:camel>],
                },
                mixin = {
                    builder   = [<$mixin_ctor:camel Builder>],
                    state_mod = [<$mixin_ctor _builder>],
                    ctor      = $mixin_ctor,
                    finish_fn = build,
                },
                ext = {
                    trait  = $ext_trait_ty,
                    method = $mixin_ctor,
                },
            );
        }
    };

    // ── Explicit form ─────────────────────────────────────────────────────────
    // Last resort: supply every identifier directly when conventions don't hold.
    (
        profile = $profile:ident,
        base = {
            builder         = $base_ty:ident,
            state_mod       = $base_state_mod:ident,
            set_method      = $base_set_method:ident,
            state_field     = $base_state_field:ident,
            state_set_field = $base_state_set_field:ident $(,)?
        },
        mixin = {
            builder   = $mixin_ty:ident,
            state_mod = $mixin_state_mod:ident,
            ctor      = $mixin_ctor:ident,
            finish_fn = $mixin_finish_fn:ident $(,)?
        },
        ext = {
            trait  = $ext_trait_ty:ident,
            method = $ext_method:ident $(,)?
        } $(,)?
    ) => {

        $crate::paste::paste! {
            mod [<__ generated_mod_ $ext_trait_ty:snake >] {
                use super::*;
                use $crate::configuration::builder::*;
                pub trait $ext_trait_ty<S: $base_state_mod::State>
                where
                    S::$base_state_field: $base_state_mod::IsUnset,
                {
                    fn $ext_method<O, F>(
                        self,
                        apply: F,
                    ) -> $base_ty<$profile, $base_state_mod::$base_state_set_field<S>>
                    where
                        O: $mixin_state_mod::IsComplete,
                        F: FnOnce($mixin_ty<$mixin_state_mod::Empty>) -> $mixin_ty<O>;
                }

                impl<S: $base_state_mod::State> $ext_trait_ty<S>
                    for $base_ty<$profile, S>
                where
                    S::$base_state_field: $base_state_mod::IsUnset,
                {
                    #[inline(always)]
                    fn $ext_method<O, F>(
                        self,
                        apply: F,
                    ) -> $base_ty<$profile, $base_state_mod::$base_state_set_field<S>>
                    where
                        O: $mixin_state_mod::IsComplete,
                        F: FnOnce($mixin_ty<$mixin_state_mod::Empty>) -> $mixin_ty<O>,
                    {
                        let mixin = $mixin_ctor();
                        let mixin = apply(mixin);
                        let mixin = mixin.$mixin_finish_fn();
                        self.$base_set_method(mixin)
                    }
                }
            }
            pub use [<__ generated_mod_ $ext_trait_ty:snake >]::*;
        }
    };
}

#[macro_export]
macro_rules! bon_builder_extend_multiple {
    // ── Minimal form ─────────────────────────────────────────────────────────
    // Infers everything, including the extension trait name, from four inputs.
    // Trait name convention: base.builder + PascalCase(mixin.ctor) + "Ext"
    //   e.g. StringBuilder + gen_base → StringBuilderGenBaseExt
    //
    // Use the short form below to supply a custom trait name.
    (
        profile = $profile:ident,
        bases = [$({
            builder    = $base_ty:ident,
            set_method = $base_set_method:ident $(,)?
        }),*$(,)?],
        mixin = {
            ctor = $mixin_ctor:ident $(,)?
        } $(,)?
    ) => {
        $(
            $crate::paste::paste! {
                $crate::bon_builder_extend!(
                    profile = $profile,
                    base = {
                        builder    = $base_ty,
                        set_method = $base_set_method,
                    },
                    mixin = {
                        ctor = $mixin_ctor,
                    },
                    ext = {
                        trait = [<$base_ty $mixin_ctor:camel Ext>],
                    },
                );
            }
        )*
    };
}

#[cfg(test)]
mod tests {

    use super::*;
    use omni_input_schema::{Input, InputKind};

    // ── boolean / confirm ──────────────────────────────────────────────────
    //
    fn value<T>(v: T) -> Option<MaybeExpr<T>> {
        Some(MaybeExpr::Value(v))
    }

    #[test]
    fn boolean_sets_name_and_default() {
        let input = boolean::<()>().name("dry_run").default(true).build();

        assert_eq!(input.base().name, "dry_run");
        assert_eq!(input.kind(), InputKind::Boolean);

        let Input::Boolean(b) = input else {
            panic!("expected Boolean")
        };
        assert_eq!(b.default, value(true));
    }

    #[test]
    fn condition_string_is_treated_as_expression() {
        let input = string::<()>()
            .name("extra")
            .condition("{{ inputs.mode == 'advanced' }}")
            .build();

        assert_eq!(
            input.base().r#if,
            Some(MaybeExpr::Expr(
                "{{ inputs.mode == 'advanced' }}".to_string()
            ))
        );
    }

    // ── text / string ──────────────────────────────────────────────────────

    #[test]
    fn text_sets_string_default() {
        let input = string::<()>().name("greeting").default("hello").build();

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
    fn string_options_accept_strings_tuples_and_builders() {
        let input = string::<()>()
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
        let input = string_array::<()>()
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
            sa.default,
            Some(vec!["rust".to_string(), "go".to_string()])
        );
    }

    // ── integer / float ───────────────────────────────────────────────────

    #[test]
    fn integer_sets_typed_default() {
        let input = integer::<()>().name("count").default(42_i64).build();
        assert_eq!(input.kind(), InputKind::Integer);
        let Input::Integer(i) = input else { panic!() };
        assert_eq!(i.default, value(42));
    }

    #[test]
    fn float_sets_typed_default() {
        let input = float::<()>().name("rate").default(1.5_f64).build();
        assert_eq!(input.kind(), InputKind::Float);
        let Input::Float(f) = input else { panic!() };
        assert!(
            (f.default.unwrap().try_as_value().unwrap() - 1.5).abs() < 1e-10
        );
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
        assert_eq!(ia.default, Some(vec![1, 2, 3]));
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
        let default = fa.default.unwrap();
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
                string::<()>().name("first_name").build(),
                string::<()>().name("last_name").build(),
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
        let input = string::<()>()
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
        let av = allowed::<()>().value("Yes").build();
        assert_eq!(av.value, "Yes");
    }

    #[test]
    fn allowed_value_accepts_explicit_value() {
        let av = allowed::<()>()
            .value("mit")
            .description("MIT License")
            .build();
        assert_eq!(av.value, "mit");
        assert_eq!(av.description.as_deref(), Some("MIT License"));
    }
}
