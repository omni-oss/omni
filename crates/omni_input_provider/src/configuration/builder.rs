//! Fluent builders for [`InputConfiguration`].
//!
//! Each input type exposes a free function (e.g. [`confirm`], [`text`]) that
//! returns a [`bon`] builder with the shared fields (`name`, `message`,
//! `condition`, `description`, ...) flattened alongside the type-specific
//! fields. Builders are finished with `.call()`:
//!
//! ```
//! use omni_input_provider::configuration::builder::confirm;
//!
//! let input = confirm::<()>()
//!     .name("dry_run")
//!     .message("Dry run?")
//!     .default(true)
//!     .build();
//! ```
//!
//! Adding a new input type only requires a new entry in the
//! [`input_builders!`] invocation below.

pub use super::*;

use either::Either;

/// A value that may either be a concrete literal (`L`) or a Tera expression
/// (a `String`).
///
/// This is the ergonomic input type for `if`/`default` builder setters: it
/// accepts both literals (`true`, `1.5`, `3`) and expressions (`"{{ ... }}"`)
/// via [`Into`], and converts to the [`Either`] representation stored in the
/// configuration structs.
#[derive(Debug, Clone, PartialEq)]
pub enum ValueOrExpr<L> {
    /// A concrete literal value.
    Value(L),
    /// A Tera expression to be evaluated at collection time.
    Expr(String),
}

impl<L> ValueOrExpr<L> {
    /// Convert into the `Either<L, String>` representation stored in the
    /// configuration structs.
    #[inline(always)]
    pub fn into_either(self) -> Either<L, String> {
        match self {
            ValueOrExpr::Value(value) => Either::Left(value),
            ValueOrExpr::Expr(expr) => Either::Right(expr),
        }
    }
}

impl<L> From<ValueOrExpr<L>> for Either<L, String> {
    #[inline(always)]
    fn from(value: ValueOrExpr<L>) -> Self {
        value.into_either()
    }
}

impl<L> From<Either<L, String>> for ValueOrExpr<L> {
    #[inline(always)]
    fn from(value: Either<L, String>) -> Self {
        match value {
            Either::Left(value) => ValueOrExpr::Value(value),
            Either::Right(expr) => ValueOrExpr::Expr(expr),
        }
    }
}

// String-like inputs are always treated as Tera expressions. These are generic
// over `L` and never overlap with the concrete `Value` conversions below
// (whose `L` is `bool`/`f64`/`i64`, never `String`/`&str`).
impl<L> From<String> for ValueOrExpr<L> {
    #[inline(always)]
    fn from(value: String) -> Self {
        ValueOrExpr::Expr(value)
    }
}

impl<L> From<&str> for ValueOrExpr<L> {
    #[inline(always)]
    fn from(value: &str) -> Self {
        ValueOrExpr::Expr(value.to_string())
    }
}

impl From<bool> for ValueOrExpr<bool> {
    #[inline(always)]
    fn from(value: bool) -> Self {
        ValueOrExpr::Value(value)
    }
}

impl From<f64> for ValueOrExpr<f64> {
    #[inline(always)]
    fn from(value: f64) -> Self {
        ValueOrExpr::Value(value)
    }
}

impl From<i64> for ValueOrExpr<i64> {
    #[inline(always)]
    fn from(value: i64) -> Self {
        ValueOrExpr::Value(value)
    }
}

// Convenience: untyped integer literals default to `i32`, so accept them too.
impl From<i32> for ValueOrExpr<i64> {
    #[inline(always)]
    fn from(value: i32) -> Self {
        ValueOrExpr::Value(value as i64)
    }
}

/// Assemble a [`BaseInputConfiguration`] from the flat shared fields shared by
/// every input builder. Single source of truth for the base layout.
#[inline(always)]
pub fn build_base(
    name: String,
    message: String,
    condition: Option<ValueOrExpr<bool>>,
    description: Option<String>,
) -> BaseInputConfiguration {
    BaseInputConfiguration::new(
        name,
        message,
        condition.map(ValueOrExpr::into_either),
        description,
    )
}

// ── OptionConfiguration ───────────────────────────────────────────────────

/// `"value"` becomes an option whose `name` and `value` are both that string.
impl From<&str> for OptionConfiguration {
    fn from(value: &str) -> Self {
        OptionConfiguration::new(value, None::<String>, value, false)
    }
}

impl From<String> for OptionConfiguration {
    fn from(value: String) -> Self {
        OptionConfiguration::new(value.clone(), None::<String>, value, false)
    }
}

/// `("Name", "value")` becomes an option with a distinct display name.
impl From<(&str, &str)> for OptionConfiguration {
    fn from((name, value): (&str, &str)) -> Self {
        OptionConfiguration::new(name, None::<String>, value, false)
    }
}

impl From<(String, String)> for OptionConfiguration {
    fn from((name, value): (String, String)) -> Self {
        OptionConfiguration::new(name, None::<String>, value, false)
    }
}

/// Build an [`OptionConfiguration`] for a `Select`/`MultiSelect` input.
///
/// `value` defaults to `name` when omitted.
#[::bon::builder(finish_fn = build)]
pub fn option(
    #[builder(into)] name: String,
    #[builder(into)] value: Option<String>,
    #[builder(into)] description: Option<String>,
    #[builder(default)] separator: bool,
) -> OptionConfiguration {
    let value = value.unwrap_or_else(|| name.clone());
    OptionConfiguration::new(name, description, value, separator)
}

// ── ValidateConfiguration ─────────────────────────────────────────────────

/// A bare string becomes a validator whose `condition` is that Tera expression.
impl From<&str> for ValidateConfiguration {
    fn from(condition: &str) -> Self {
        ValidateConfiguration::new(
            ValueOrExpr::<bool>::from(condition).into_either(),
            None::<String>,
        )
    }
}

impl From<String> for ValidateConfiguration {
    fn from(condition: String) -> Self {
        ValidateConfiguration::new(
            ValueOrExpr::<bool>::from(condition).into_either(),
            None::<String>,
        )
    }
}

/// `("{{ expr }}", "error message")` pairs a condition with its error message.
impl From<(&str, &str)> for ValidateConfiguration {
    fn from((condition, error_message): (&str, &str)) -> Self {
        ValidateConfiguration::new(
            ValueOrExpr::<bool>::from(condition).into_either(),
            Some(error_message.to_string()),
        )
    }
}

/// Build a [`ValidateConfiguration`] for a validated input.
///
/// `condition` accepts a literal `bool` or a Tera expression string.
#[::bon::builder(finish_fn = build)]
pub fn validator(
    #[builder(into)] condition: ValueOrExpr<bool>,
    #[builder(into)] error_message: Option<String>,
) -> ValidateConfiguration {
    ValidateConfiguration::new(condition.into_either(), error_message)
}

/// Generates a single `#[bon::builder]` function plus the matching
/// `From<InnerConfig> for InputConfiguration<TExtra>` impl.
///
/// Two shapes are supported:
/// - `@base`: the inner config wraps a [`BaseInputConfiguration`] directly.
/// - `@validated`: the inner config wraps a [`ValidatedInputConfiguration`]
///   (base + `validate`), so a `validate` setter is added.
///
/// Type-specific fields are listed as `name: Type => conversion_expr`, where
/// `conversion_expr` maps the builder parameter into the argument expected by
/// the inner config's derive-new `::new` constructor (in declaration order).
macro_rules! __input_builder_fn {
    // ── base-shaped inputs ────────────────────────────────────────────────
    (
        $(#[$fn_meta:meta])*
        $vis:vis fn $fn:ident -> $variant:ident : $config:ty , @base {
            $( $(#[$p_meta:meta])* $p_name:ident : $p_ty:ty => $conv:expr ),* $(,)?
        }
    ) => {
        impl<TExtra: InputExtras> From<$config> for InputConfiguration<TExtra> {
            fn from(input: $config) -> Self {
                InputConfiguration::$variant { input, extra: TExtra::default() }
            }
        }

        #[::bon::builder(finish_fn = build)]
        $(#[$fn_meta])*
        $vis fn $fn<TExtra: InputExtras>(
            #[builder(into)] name: String,
            #[builder(into)] message: String,
            #[builder(into)] condition: Option<ValueOrExpr<bool>>,
            #[builder(into)] description: Option<String>,
            $( $(#[$p_meta])* $p_name: $p_ty, )*
            #[builder(default)] extra: TExtra,
        ) -> InputConfiguration<TExtra> {
            let base = build_base(name, message, condition, description);
            let input = <$config>::new(base $(, $conv)*);
            InputConfiguration::$variant { input, extra }
        }
    };

    // ── validated-shaped inputs ───────────────────────────────────────────
    (
        $(#[$fn_meta:meta])*
        $vis:vis fn $fn:ident -> $variant:ident : $config:ty , @validated {
            $( $(#[$p_meta:meta])* $p_name:ident : $p_ty:ty => $conv:expr ),* $(,)?
        }
    ) => {
        impl<TExtra: InputExtras> From<$config> for InputConfiguration<TExtra> {
            fn from(input: $config) -> Self {
                InputConfiguration::$variant { input, extra: TExtra::default() }
            }
        }

        #[::bon::builder(finish_fn = build)]
        $(#[$fn_meta])*
        $vis fn $fn<TExtra: InputExtras>(
            #[builder(into)] name: String,
            #[builder(into)] message: String,
            #[builder(into)] condition: Option<ValueOrExpr<bool>>,
            #[builder(into)] description: Option<String>,
            #[builder(default, with = |validators: impl IntoIterator<Item = impl Into<ValidateConfiguration>>|
                validators.into_iter().map(Into::into).collect())]
            validate: Vec<ValidateConfiguration>,
            $( $(#[$p_meta])* $p_name: $p_ty, )*
            #[builder(default)] extra: TExtra,
        ) -> InputConfiguration<TExtra> {
            let base = build_base(name, message, condition, description);
            let base = ValidatedInputConfiguration::new(base, validate);
            let input = <$config>::new(base $(, $conv)*);
            InputConfiguration::$variant { input, extra }
        }
    };
}

/// Generates every input builder. Add a new input type by adding one entry.
macro_rules! input_builders {
    (
        $(
            $(#[$fn_meta:meta])*
            $vis:vis fn $fn:ident -> $variant:ident : $config:ty , @ $kind:ident {
                $( $(#[$p_meta:meta])* $p_name:ident : $p_ty:ty => $conv:expr ),* $(,)?
            }
        )*
    ) => {
        $(
            __input_builder_fn! {
                $(#[$fn_meta])*
                $vis fn $fn -> $variant : $config , @ $kind {
                    $( $(#[$p_meta])* $p_name : $p_ty => $conv ),*
                }
            }
        )*
    };
}

input_builders! {
    /// Build a `Confirm` input.
    pub fn confirm -> Confirm : ConfirmInputConfiguration , @base {
        #[builder(into)] default: Option<ValueOrExpr<bool>>
            => default.map(ValueOrExpr::into_either),
    }

    /// Build a `Select` input.
    pub fn select -> Select : SelectInputConfiguration , @base {
        #[builder(with = |options: impl IntoIterator<Item = impl Into<OptionConfiguration>>|
            options.into_iter().map(Into::into).collect())]
        options: Vec<OptionConfiguration> => options,
        #[builder(into)] default: Option<String> => default,
    }

    /// Build a `MultiSelect` input.
    pub fn multi_select -> MultiSelect : MultiSelectInputConfiguration , @validated {
        #[builder(with = |options: impl IntoIterator<Item = impl Into<OptionConfiguration>>|
            options.into_iter().map(Into::into).collect())]
        options: Vec<OptionConfiguration> => options,
        #[builder(with = |defaults: impl IntoIterator<Item = impl Into<String>>|
            defaults.into_iter().map(Into::into).collect())]
        default: Option<Vec<String>> => default,
    }

    /// Build a `Text` input.
    pub fn text -> Text : TextInputConfiguration , @validated {
        #[builder(into)] default: Option<String> => default,
    }

    /// Build a `Password` input.
    pub fn password -> Password : PasswordInputConfiguration , @validated {}

    /// Build a `Float` input.
    pub fn float -> Float : FloatInputConfiguration , @validated {
        #[builder(into)] default: Option<ValueOrExpr<f64>>
            => default.map(ValueOrExpr::into_either),
    }

    /// Build an `Integer` input.
    pub fn integer -> Integer : IntegerInputConfiguration , @validated {
        #[builder(into)] default: Option<ValueOrExpr<i64>>
            => default.map(ValueOrExpr::into_either),
    }
}

#[cfg(test)]
mod tests {
    use either::Either;

    use super::*;

    #[test]
    fn confirm_sets_shared_and_literal_default() {
        let input = confirm::<()>()
            .name("dry_run")
            .message("Dry run?")
            .description("Skip side effects")
            .default(true)
            .build();

        assert_eq!(input.name(), "dry_run");
        assert_eq!(input.message(), "Dry run?");
        assert_eq!(input.description(), Some("Skip side effects"));
        assert_eq!(input.condition(), None);

        let InputConfiguration::Confirm { input, .. } = input else {
            panic!("expected a confirm input");
        };
        assert_eq!(input.default, Some(Either::Left(true)));
    }

    #[test]
    fn condition_string_is_treated_as_expression() {
        let input = text::<()>()
            .name("extra")
            .message("Extra?")
            .condition("{{ inputs.mode == 'advanced' }}")
            .build();

        assert_eq!(
            input.condition(),
            Some(&Either::Right(
                "{{ inputs.mode == 'advanced' }}".to_string()
            ))
        );
    }

    #[test]
    fn numeric_defaults_accept_literals_and_expressions() {
        let float = float::<()>()
            .name("ratio")
            .message("Ratio")
            .default(1.5)
            .build();
        let InputConfiguration::Float { input, .. } = float else {
            panic!("expected a float input");
        };
        assert_eq!(input.default, Some(Either::Left(1.5)));

        let integer = integer::<()>()
            .name("count")
            .message("Count")
            .default("{{ inputs.size }}")
            .build();
        let InputConfiguration::Integer { input, .. } = integer else {
            panic!("expected an integer input");
        };
        assert_eq!(
            input.default,
            Some(Either::Right("{{ inputs.size }}".to_string()))
        );
    }

    #[test]
    fn validated_inputs_collect_validators() {
        let input = text::<()>()
            .name("name")
            .message("Name")
            .validate(vec![ValidateConfiguration::new(
                Either::<bool, String>::Right(
                    "{{ value | length > 0 }}".into(),
                ),
                Some("required".to_string()),
            )])
            .build();

        let InputConfiguration::Text { input, .. } = input else {
            panic!("expected a text input");
        };
        assert_eq!(input.base.validate.len(), 1);
    }

    #[test]
    fn validate_accepts_strings_tuples_and_builders() {
        let input = text::<()>()
            .name("name")
            .message("Name")
            .validate([
                // bare expression string
                ValidateConfiguration::from("{{ value | length > 0 }}"),
                // (expression, error message) tuple
                ("{{ value | length < 20 }}", "too long").into(),
                // explicit builder
                validator()
                    .condition("{{ value != 'admin' }}")
                    .error_message("reserved")
                    .build(),
            ])
            .build();

        let InputConfiguration::Text { input, .. } = input else {
            panic!("expected a text input");
        };
        assert_eq!(input.base.validate.len(), 3);
        assert_eq!(
            input.base.validate[1].error_message.as_deref(),
            Some("too long")
        );
    }

    #[test]
    fn select_options_accept_strings_tuples_and_builders() {
        let input = select::<()>()
            .name("color")
            .message("Pick a color")
            .options([
                // name == value
                OptionConfiguration::from("red"),
                // (name, value)
                ("Bright Green", "green").into(),
                // explicit builder with a description
                option()
                    .name("Blue")
                    .value("blue")
                    .description("the calm one")
                    .build(),
            ])
            .default("red")
            .build();

        let InputConfiguration::Select { input, .. } = input else {
            panic!("expected a select input");
        };
        assert_eq!(input.options.len(), 3);
        assert_eq!(input.options[0].name, "red");
        assert_eq!(input.options[0].value, "red");
        assert_eq!(input.options[1].name, "Bright Green");
        assert_eq!(input.options[1].value, "green");
        assert_eq!(
            input.options[2].description.as_deref(),
            Some("the calm one")
        );
        assert_eq!(input.default.as_deref(), Some("red"));
    }

    #[test]
    fn multi_select_default_accepts_an_iterator_of_strings() {
        let input = multi_select::<()>()
            .name("langs")
            .message("Languages")
            .options(["rust", "go", "zig"])
            .default(["rust", "go"])
            .build();

        let InputConfiguration::MultiSelect { input, .. } = input else {
            panic!("expected a multi-select input");
        };
        assert_eq!(input.options.len(), 3);
        assert_eq!(
            input.default,
            Some(vec!["rust".to_string(), "go".to_string()])
        );
    }

    #[test]
    fn option_value_defaults_to_name() {
        let opt = option().name("Yes").build();
        assert_eq!(opt.name, "Yes");
        assert_eq!(opt.value, "Yes");
    }

    #[test]
    fn inner_config_converts_into_enum() {
        let inner = ConfirmInputConfiguration::new(
            BaseInputConfiguration::new("ok", "OK?", None, None),
            None::<Either<bool, String>>,
        );
        let input: InputConfiguration<()> = inner.into();
        assert!(matches!(input, InputConfiguration::Confirm { .. }));
    }
}
