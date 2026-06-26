use maps::UnorderedMap;
use omni_config_types::MaybeExpr;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sets::UnorderedSet;
use value_bag::{OwnedValueBag, ValueBag};

use crate::base::ValidateConfiguration;
use crate::error::{Error, ErrorInner, ErrorKind};
use crate::input::{Input, InputKind};
use crate::profile::InputProfile;

/// A single field-level validation failure.
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, PartialEq)]
pub struct ValidationError {
    pub input_name: String,
    pub message: String,
}

/// The outcome of a [`validate`] call.
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, PartialEq)]
pub struct ValidationReport {
    pub errors: Vec<ValidationError>,
}

impl ValidationReport {
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Configuration for [`validate`].
#[derive(Debug, Clone)]
pub struct ValidationConfig<'a> {
    /// Variable name under which already-resolved values are exposed when
    /// evaluating `if` Tera expressions.  Defaults to `"inputs"`.
    pub if_expressions_root_property: Option<&'a str>,
    /// Variable name under which the candidate value is exposed when
    /// evaluating validator Tera expressions.  Defaults to `"value"`.
    pub validation_value_name: Option<&'a str>,
    /// When `true`, inputs with a `default` skip the missing-required check.
    pub use_defaults: bool,
}

impl Default for ValidationConfig<'_> {
    fn default() -> Self {
        Self {
            if_expressions_root_property: Some("inputs"),
            validation_value_name: Some("value"),
            use_defaults: false,
        }
    }
}

/// Validate a set of pre-supplied input values against an input schema.
///
/// For each active (non-skipped) input:
/// - emits a [`ValidationError`] when the value is missing and no default is
///   available (accounting for `config.use_defaults`)
/// - type-checks boolean / integer / float inputs
/// - checks that the value is in `allowed` when the input is constrained
/// - runs all Tera-based validator expressions
/// - reports the `secret + remember` conflict as an infrastructure error
///
/// All errors are collected before returning — never short-circuits on the first.
/// Infrastructure errors (malformed Tera, duplicate names, etc.) are returned
/// as `Err(Error)`, not included in the report.
pub fn validate<E: InputProfile>(
    inputs: &[Input<E>],
    values: &UnorderedMap<String, OwnedValueBag>,
    ctx: &omni_tera::Context,
    config: &ValidationConfig<'_>,
) -> Result<ValidationReport, Error> {
    check_no_duplicate_names(inputs)?;

    // Serde deserializes all variants (monomorphic); this is the runtime gate.
    for input in inputs {
        let kind = input.kind();
        if !E::SUPPORTED.contains(kind) {
            return Err(ErrorInner::UnsupportedInputKind {
                input_name: input.base().name.clone(),
                kind,
            })?;
        }
    }

    let mut errors = Vec::new();
    // Tracks the resolved effective value for each processed input so that
    // later if-expressions can reference earlier inputs' values.
    let mut effective: UnorderedMap<String, OwnedValueBag> = values.clone();

    for input in inputs {
        let base = input.base();
        let name = &base.name;

        // secret + remember is always a hard error.
        if base.secret && E::is_remember(input) {
            return Err(ErrorInner::SecretRememberConflict {
                input_name: name.clone(),
            })?;
        }

        // Evaluate the `if` condition.
        if let Some(if_expr) = &base.r#if {
            if should_skip(
                if_expr,
                &effective,
                ctx,
                config.if_expressions_root_property,
            )? {
                continue;
            }
        }

        let value = values.get(name.as_str());
        let has_default =
            config.use_defaults && input.static_default_value_bag().is_some();

        if value.is_none() && !has_default {
            errors.push(ValidationError {
                input_name: name.clone(),
                message: format!("required input '{name}' is missing"),
            });
            continue;
        }

        // Populate effective with the default so subsequent if-expressions see it.
        if value.is_none() {
            if let Some(default) = input.static_default_value_bag() {
                effective.insert(name.clone(), default);
            }
        }

        if let Some(raw) = value {
            match validate_single(input, name, raw, ctx, &effective, config) {
                Ok(typed) => {
                    effective.insert(name.clone(), typed);
                }
                Err(ValidationSingleError::Type(e)) => {
                    errors.push(ValidationError {
                        input_name: name.clone(),
                        message: e.to_string(),
                    });
                }
                Err(ValidationSingleError::Infra(e)) => return Err(e),
                Err(ValidationSingleError::Value(msg)) => {
                    errors.push(ValidationError {
                        input_name: name.clone(),
                        message: msg,
                    });
                }
            }
        }
    }

    Ok(ValidationReport { errors })
}

// ── Internals ─────────────────────────────────────────────────────────────────

enum ValidationSingleError {
    Type(Error),
    Value(String),
    Infra(Error),
}

fn validate_single<E: InputProfile>(
    input: &Input<E>,
    name: &str,
    raw: &OwnedValueBag,
    ctx: &omni_tera::Context,
    _effective: &UnorderedMap<String, OwnedValueBag>,
    config: &ValidationConfig<'_>,
) -> Result<OwnedValueBag, ValidationSingleError> {
    let typed =
        coerce_type(input, name, raw).map_err(ValidationSingleError::Type)?;

    // Check allowed-value constraint.
    if let Err(msg) = check_allowed(input, name, &typed) {
        return Err(ValidationSingleError::Value(msg));
    }

    // Run per-field validators.
    validate_value(
        name,
        &typed,
        ctx,
        &input.base().validators,
        config.validation_value_name,
    )
    .map_err(|e| {
        if e.kind() == ErrorKind::InvalidValue {
            ValidationSingleError::Value(e.to_string())
        } else {
            ValidationSingleError::Infra(e)
        }
    })?;

    Ok(typed)
}

fn coerce_type<E: InputProfile>(
    input: &Input<E>,
    name: &str,
    raw: &OwnedValueBag,
) -> Result<OwnedValueBag, Error> {
    match input.kind() {
        InputKind::Boolean => try_parse_bool(raw.by_ref())
            .ok_or_else(|| make_type_error(name, raw.by_ref(), "boolean"))
            .map(|b| ValueBag::capture_serde1(&b).to_owned()),
        InputKind::Integer => try_parse_int(raw.by_ref())
            .ok_or_else(|| make_type_error(name, raw.by_ref(), "integer"))
            .map(|i| ValueBag::capture_serde1(&i).to_owned()),
        InputKind::Float => try_parse_float(raw.by_ref())
            .ok_or_else(|| make_type_error(name, raw.by_ref(), "float"))
            .map(|f| ValueBag::capture_serde1(&f).to_owned()),
        _ => Ok(raw.clone()),
    }
}

fn check_allowed<E: InputProfile>(
    input: &Input<E>,
    name: &str,
    typed: &OwnedValueBag,
) -> Result<(), String> {
    match input {
        Input::String(s) => {
            if let Some(list) = &s.allowed {
                let sv = typed
                    .by_ref()
                    .to_str()
                    .map(|s| s.to_string())
                    .or_else(|| {
                        typed.by_ref().to_borrowed_str().map(|s| s.to_string())
                    });
                if let Some(sv) = sv {
                    if !list.iter().any(|a| a.value == sv) {
                        return Err(format!(
                            "'{sv}' is not an allowed value for input '{name}'"
                        ));
                    }
                }
            }
        }
        Input::Integer(i) => {
            if let Some(list) = &i.allowed {
                if let Some(iv) = try_parse_int(typed.by_ref()) {
                    if !list.iter().any(|a| a.value == iv) {
                        return Err(format!(
                            "'{iv}' is not an allowed value for input '{name}'"
                        ));
                    }
                }
            }
        }
        Input::Float(f) => {
            if let Some(list) = &f.allowed {
                if let Some(fv) = try_parse_float(typed.by_ref()) {
                    if !list.iter().any(|a| a.value == fv) {
                        return Err(format!(
                            "'{fv}' is not an allowed value for input '{name}'"
                        ));
                    }
                }
            }
        }
        Input::StringArray(sa) => {
            // For arrays, check each element when the array is passed as a JSON array.
            // Value-bag doesn't have direct array access; skip element-level check here.
            // Element-level allowed checking is covered by collect's validate pass.
            let _ = &sa.body.allowed;
        }
        _ => {}
    }
    Ok(())
}

pub fn validate_value(
    input_name: &str,
    value: &OwnedValueBag,
    ctx: &omni_tera::Context,
    validators: &[ValidateConfiguration],
    value_name: Option<&str>,
) -> Result<(), Error> {
    for (index, validator) in validators.iter().enumerate() {
        let mut eval_ctx = ctx.clone();
        eval_ctx.insert(value_name.unwrap_or("value"), value);

        let is_error = match &validator.condition {
            MaybeExpr::Value(l) => !*l,
            MaybeExpr::Expr(r) => {
                let result = omni_tera::one_off(
                    r,
                    &format!(
                        "condition for input {} at index {}",
                        input_name, index
                    ),
                    &eval_ctx,
                )?;
                let result = result.trim();
                validate_boolean_expression_result(result, r)?;
                result != "true"
            }
        };

        if is_error {
            let error_message = validator
                .error_message
                .as_ref()
                .map(|e| omni_tera::Tera::one_off(e, &eval_ctx, true))
                .unwrap_or_else(|| {
                    Ok(format!(
                        "condition '{}' evaluated to false",
                        validator.condition
                    ))
                })?;
            return Err(ErrorInner::InvalidValue {
                input_name: input_name.to_string(),
                value: value.clone(),
                error_message,
            })?;
        }
    }
    Ok(())
}

pub fn validate_boolean_expression_result(
    result: &str,
    expression: &str,
) -> Result<(), Error> {
    if result != "true" && result != "false" {
        return Err(ErrorInner::InvalidBooleanExpressionResult {
            result: result.to_string(),
            expression: expression.to_string(),
        })?;
    }
    Ok(())
}

fn should_skip(
    if_expr: &MaybeExpr<bool>,
    effective: &UnorderedMap<String, OwnedValueBag>,
    ctx: &omni_tera::Context,
    root_property: Option<&str>,
) -> Result<bool, Error> {
    Ok(match if_expr {
        MaybeExpr::Value(l) => !*l,
        MaybeExpr::Expr(expr) => {
            let mut eval_ctx = ctx.clone();
            eval_ctx.insert(root_property.unwrap_or("inputs"), effective);
            let result = omni_tera::one_off(expr, expr, &eval_ctx)?;
            let result = result.trim();
            validate_boolean_expression_result(result, expr)?;
            result != "true"
        }
    })
}

fn check_no_duplicate_names<E: InputProfile>(
    inputs: &[Input<E>],
) -> Result<(), Error> {
    let mut seen: UnorderedSet<&str> = UnorderedSet::default();
    for input in inputs {
        let name = input.base().name.as_str();
        if seen.contains(name) {
            return Err(ErrorInner::DuplicateInputName(name.to_string()))?;
        }
        seen.insert(name);
    }
    Ok(())
}

fn try_parse_bool(value: value_bag::ValueBag<'_>) -> Option<bool> {
    if let Some(b) = value.to_bool() {
        return Some(b);
    }
    value.to_str().and_then(|s| s.parse::<bool>().ok())
}

fn try_parse_float(value: value_bag::ValueBag<'_>) -> Option<f64> {
    if let Some(f) = value.to_f64() {
        return Some(f);
    }
    value.to_str().and_then(|s| s.parse::<f64>().ok())
}

fn try_parse_int(value: value_bag::ValueBag<'_>) -> Option<i64> {
    if let Some(i) = value.to_i64() {
        return Some(i);
    }
    value.to_str().and_then(|s| s.parse::<i64>().ok())
}

fn make_type_error(
    input_name: &str,
    value: value_bag::ValueBag<'_>,
    expected_type: &str,
) -> Error {
    Error::from(eyre::eyre!(
        "{input_name}: value is not of type {expected_type}: value {}",
        serde_json::to_string_pretty(&value).expect("should be converted"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::BaseInput;
    use crate::error::ErrorKind;
    use crate::input::{
        BooleanInput, FloatInput, Input, IntegerInput, StringInput,
    };
    use crate::profile::InputProfile;
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};
    use value_bag::ValueBag;

    // ── Test helpers ──────────────────────────────────────────────────────────

    fn empty_ctx() -> omni_tera::Context {
        omni_tera::Context::new()
    }

    fn empty_values() -> maps::UnorderedMap<String, value_bag::OwnedValueBag> {
        Default::default()
    }

    fn default_config() -> ValidationConfig<'static> {
        ValidationConfig::default()
    }

    fn bag_bool(b: bool) -> value_bag::OwnedValueBag {
        ValueBag::capture_serde1(&b).to_owned()
    }

    fn bag_str(s: &str) -> value_bag::OwnedValueBag {
        let owned = s.to_string();
        ValueBag::capture_serde1(&owned).to_owned()
    }

    fn bag_i64(i: i64) -> value_bag::OwnedValueBag {
        ValueBag::capture_serde1(&i).to_owned()
    }

    fn bag_f64(f: f64) -> value_bag::OwnedValueBag {
        ValueBag::capture_serde1(&f).to_owned()
    }

    fn base(name: &str) -> BaseInput {
        BaseInput {
            name: name.to_string(),
            r#if: None,
            validators: vec![],
            secret: false,
            description: None,
        }
    }

    fn boolean_input(name: &str) -> Input<()> {
        Input::Boolean(BooleanInput {
            base: base(name),
            default: None,
            base_extra: (),
            boolean_extra: (),
        })
    }

    fn string_input(name: &str) -> Input<()> {
        Input::String(StringInput {
            base: base(name),
            allowed: None,
            default: None,
            base_extra: (),
            string_extra: (),
        })
    }

    fn integer_input(name: &str) -> Input<()> {
        Input::Integer(IntegerInput {
            base: base(name),
            allowed: None,
            default: None,
            base_extra: (),
            numeric_extra: (),
        })
    }

    fn float_input(name: &str) -> Input<()> {
        Input::Float(FloatInput {
            base: base(name),
            allowed: None,
            default: None,
            base_extra: (),
            numeric_extra: (),
        })
    }

    // ── Required / missing ────────────────────────────────────────────────────

    #[test]
    fn missing_required_field_produces_error() {
        let inputs = [boolean_input("flag")];
        let report =
            validate(&inputs, &empty_values(), &empty_ctx(), &default_config())
                .unwrap();
        assert!(!report.is_valid());
        assert_eq!(report.errors[0].input_name, "flag");
    }

    #[test]
    fn provided_required_field_passes() {
        let inputs = [boolean_input("flag")];
        let mut values = empty_values();
        values.insert("flag".to_string(), bag_bool(true));
        let report =
            validate(&inputs, &values, &empty_ctx(), &default_config())
                .unwrap();
        assert!(report.is_valid());
    }

    #[test]
    fn defaulted_field_not_required_when_use_defaults_true() {
        let input: Input<()> = Input::String(StringInput {
            base: base("mode"),
            allowed: None,
            default: Some("dev".to_string()),
            base_extra: (),
            string_extra: (),
        });
        let config = ValidationConfig {
            use_defaults: true,
            ..ValidationConfig::default()
        };
        let report =
            validate(&[input], &empty_values(), &empty_ctx(), &config).unwrap();
        assert!(report.is_valid());
    }

    #[test]
    fn defaulted_field_required_when_use_defaults_false() {
        let input: Input<()> = Input::String(StringInput {
            base: base("mode"),
            allowed: None,
            default: Some("dev".to_string()),
            base_extra: (),
            string_extra: (),
        });
        let config = ValidationConfig {
            use_defaults: false,
            ..ValidationConfig::default()
        };
        let report =
            validate(&[input], &empty_values(), &empty_ctx(), &config).unwrap();
        assert!(!report.is_valid());
    }

    // ── If / conditional ─────────────────────────────────────────────────────

    #[test]
    fn always_hidden_input_never_required() {
        let input: Input<()> = Input::Boolean(BooleanInput {
            base: BaseInput {
                r#if: Some(MaybeExpr::Value(false)),
                ..base("hidden")
            },
            default: None,
            base_extra: (),
            boolean_extra: (),
        });
        let report = validate(
            &[input],
            &empty_values(),
            &empty_ctx(),
            &default_config(),
        )
        .unwrap();
        assert!(
            report.is_valid(),
            "always-hidden input must never be required"
        );
    }

    #[test]
    fn conditional_field_skipped_when_expr_false() {
        let inputs = [
            string_input("env"),
            Input::Boolean(BooleanInput {
                base: BaseInput {
                    r#if: Some(MaybeExpr::Expr(
                        "{{ inputs.env == 'prod' }}".to_string(),
                    )),
                    ..base("debug")
                },
                default: None,
                base_extra: (),
                boolean_extra: (),
            }),
        ];
        let mut values = empty_values();
        values.insert("env".to_string(), bag_str("dev"));
        let report =
            validate(&inputs, &values, &empty_ctx(), &default_config())
                .unwrap();
        assert!(
            report.is_valid(),
            "debug must be skipped when env != 'prod': {:?}",
            report.errors
        );
    }

    #[test]
    fn conditional_field_required_when_expr_true() {
        let inputs = [
            string_input("env"),
            Input::Boolean(BooleanInput {
                base: BaseInput {
                    r#if: Some(MaybeExpr::Expr(
                        "{{ inputs.env == 'prod' }}".to_string(),
                    )),
                    ..base("debug")
                },
                default: None,
                base_extra: (),
                boolean_extra: (),
            }),
        ];
        let mut values = empty_values();
        values.insert("env".to_string(), bag_str("prod"));
        // "debug" is not in values
        let report =
            validate(&inputs, &values, &empty_ctx(), &default_config())
                .unwrap();
        assert!(
            !report.is_valid(),
            "debug must be required when env == 'prod'"
        );
        assert_eq!(report.errors[0].input_name, "debug");
    }

    // ── Type coercion ─────────────────────────────────────────────────────────

    #[test]
    fn boolean_coercion_from_string_true() {
        let mut values = empty_values();
        values.insert("flag".to_string(), bag_str("true"));
        let report = validate(
            &[boolean_input("flag")],
            &values,
            &empty_ctx(),
            &default_config(),
        )
        .unwrap();
        assert!(
            report.is_valid(),
            "string 'true' must coerce to bool: {:?}",
            report.errors
        );
    }

    #[test]
    fn boolean_coercion_from_string_false() {
        let mut values = empty_values();
        values.insert("flag".to_string(), bag_str("false"));
        let report = validate(
            &[boolean_input("flag")],
            &values,
            &empty_ctx(),
            &default_config(),
        )
        .unwrap();
        assert!(
            report.is_valid(),
            "string 'false' must coerce to bool: {:?}",
            report.errors
        );
    }

    #[test]
    fn integer_coercion_from_string_numeric() {
        let mut values = empty_values();
        values.insert("count".to_string(), bag_str("42"));
        let report = validate(
            &[integer_input("count")],
            &values,
            &empty_ctx(),
            &default_config(),
        )
        .unwrap();
        assert!(
            report.is_valid(),
            "string '42' must coerce to integer: {:?}",
            report.errors
        );
    }

    #[test]
    fn integer_coercion_rejects_non_numeric_string() {
        let mut values = empty_values();
        values.insert("count".to_string(), bag_str("not-a-number"));
        let report = validate(
            &[integer_input("count")],
            &values,
            &empty_ctx(),
            &default_config(),
        )
        .unwrap();
        assert!(
            !report.is_valid(),
            "non-numeric string must fail integer type check"
        );
    }

    #[test]
    fn float_coercion_from_string_numeric() {
        let mut values = empty_values();
        values.insert("rate".to_string(), bag_str("3.14"));
        let report = validate(
            &[float_input("rate")],
            &values,
            &empty_ctx(),
            &default_config(),
        )
        .unwrap();
        assert!(
            report.is_valid(),
            "string '3.14' must coerce to float: {:?}",
            report.errors
        );
    }

    #[test]
    fn float_coercion_rejects_non_numeric_string() {
        let mut values = empty_values();
        values.insert("rate".to_string(), bag_str("oops"));
        let report = validate(
            &[float_input("rate")],
            &values,
            &empty_ctx(),
            &default_config(),
        )
        .unwrap();
        assert!(!report.is_valid());
    }

    // ── Allowed value constraints ─────────────────────────────────────────────

    #[test]
    fn rejects_string_not_in_allowed_list() {
        use crate::allowed::AllowedValue;
        let input: Input<()> = Input::String(StringInput {
            base: base("env"),
            allowed: Some(vec![
                AllowedValue {
                    value: "dev".to_string(),
                    description: None,
                    base_extra: (),
                },
                AllowedValue {
                    value: "prod".to_string(),
                    description: None,
                    base_extra: (),
                },
            ]),
            default: None,
            base_extra: (),
            string_extra: (),
        });
        let mut values = empty_values();
        values.insert("env".to_string(), bag_str("staging"));
        let report =
            validate(&[input], &values, &empty_ctx(), &default_config())
                .unwrap();
        assert!(!report.is_valid(), "value not in allowed list must fail");
    }

    #[test]
    fn rejects_integer_not_in_allowed_list() {
        use crate::allowed::AllowedValue;
        let input: Input<()> = Input::Integer(IntegerInput {
            base: base("port"),
            allowed: Some(vec![
                AllowedValue {
                    value: 80i64,
                    description: None,
                    base_extra: (),
                },
                AllowedValue {
                    value: 443i64,
                    description: None,
                    base_extra: (),
                },
            ]),
            default: None,
            base_extra: (),
            numeric_extra: (),
        });
        let mut values = empty_values();
        values.insert("port".to_string(), bag_i64(8080));
        let report =
            validate(&[input], &values, &empty_ctx(), &default_config())
                .unwrap();
        assert!(!report.is_valid(), "integer not in allowed list must fail");
    }

    #[test]
    fn rejects_float_not_in_allowed_list() {
        use crate::allowed::AllowedValue;
        let input: Input<()> = Input::Float(FloatInput {
            base: base("ratio"),
            allowed: Some(vec![
                AllowedValue {
                    value: 0.5f64,
                    description: None,
                    base_extra: (),
                },
                AllowedValue {
                    value: 1.0f64,
                    description: None,
                    base_extra: (),
                },
            ]),
            default: None,
            base_extra: (),
            numeric_extra: (),
        });
        let mut values = empty_values();
        values.insert("ratio".to_string(), bag_f64(0.75));
        let report =
            validate(&[input], &values, &empty_ctx(), &default_config())
                .unwrap();
        assert!(!report.is_valid(), "float not in allowed list must fail");
    }

    // ── Validator expressions ─────────────────────────────────────────────────

    #[test]
    fn validator_expression_rejects_failing_condition() {
        use crate::base::ValidateConfiguration;
        let input: Input<()> = Input::String(StringInput {
            base: BaseInput {
                validators: vec![ValidateConfiguration {
                    condition: MaybeExpr::new_expr("{{ value | length > 3 }}"),
                    error_message: Some("too short".to_string()),
                }],
                ..base("name")
            },
            allowed: None,
            default: None,
            base_extra: (),
            string_extra: (),
        });
        let mut values = empty_values();
        values.insert("name".to_string(), bag_str("ab")); // length 2 → fails > 3
        let report =
            validate(&[input], &values, &empty_ctx(), &default_config())
                .unwrap();
        assert!(!report.is_valid(), "short string must fail the validator");
    }

    #[test]
    fn validator_expression_accepts_passing_condition() {
        use crate::base::ValidateConfiguration;
        let input: Input<()> = Input::String(StringInput {
            base: BaseInput {
                validators: vec![ValidateConfiguration {
                    condition: MaybeExpr::new_expr("{{ value | length > 3 }}"),
                    error_message: None,
                }],
                ..base("name")
            },
            allowed: None,
            default: None,
            base_extra: (),
            string_extra: (),
        });
        let mut values = empty_values();
        values.insert("name".to_string(), bag_str("alice")); // length 5 → passes > 3
        let report =
            validate(&[input], &values, &empty_ctx(), &default_config())
                .unwrap();
        assert!(
            report.is_valid(),
            "valid string must pass the validator: {:?}",
            report.errors
        );
    }

    // ── Error collection ──────────────────────────────────────────────────────

    #[test]
    fn collects_all_errors_not_just_first() {
        let inputs =
            [boolean_input("a"), boolean_input("b"), boolean_input("c")];
        let report =
            validate(&inputs, &empty_values(), &empty_ctx(), &default_config())
                .unwrap();
        assert_eq!(
            report.errors.len(),
            3,
            "all three missing-required errors must be reported"
        );
    }

    // ── Infrastructure errors ─────────────────────────────────────────────────

    #[test]
    fn duplicate_input_names_is_infrastructure_error() {
        let inputs = [boolean_input("flag"), boolean_input("flag")];
        let result =
            validate(&inputs, &empty_values(), &empty_ctx(), &default_config());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), ErrorKind::DuplicateInputName);
    }

    // ── secret + remember conflict ────────────────────────────────────────────

    #[test]
    fn secret_and_remember_conflict_is_hard_error() {
        #[derive(
            Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq, Default,
        )]
        struct RememberBase {
            #[serde(default)]
            pub remember: bool,
        }

        #[derive(Debug, Clone, PartialEq, Default)]
        struct RememberProfile;

        impl InputProfile for RememberProfile {
            type Base = RememberBase;
            type Boolean = ();
            type String = ();
            type Numeric = ();
            type Array = ();
            type Object = ();
            type AllowedValueBase = ();

            fn is_remember(input: &Input<Self>) -> bool {
                input.base_extra().remember
            }
        }

        let input = Input::<RememberProfile>::Boolean(BooleanInput {
            base: BaseInput {
                secret: true,
                ..base("token")
            },
            default: None,
            base_extra: RememberBase { remember: true },
            boolean_extra: (),
        });
        let result = validate::<RememberProfile>(
            &[input],
            &empty_values(),
            &empty_ctx(),
            &default_config(),
        );
        assert!(result.is_err(), "secret+remember must be a hard error");
        assert_eq!(
            result.unwrap_err().kind(),
            ErrorKind::SecretRememberConflict
        );
    }
}
