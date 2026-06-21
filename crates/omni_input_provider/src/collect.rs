use crate::{
    configuration::{
        CollectionConfig, InputConfiguration, InputExtras, InputType,
        ValidateConfiguration,
    },
    error::{Error, ErrorInner, ErrorKind},
    provider::InputProvider,
    utils::{validate_boolean_expression_result, validate_value},
};
use either::Either;
use maps::{UnorderedMap, unordered_map};
use sets::UnorderedSet;
use strum::IntoDiscriminant as _;
use value_bag::{OwnedValueBag, ValueBag};

pub async fn collect<TExtra: InputExtras>(
    inputs: &[InputConfiguration<TExtra>],
    pre_exec_values: &UnorderedMap<String, OwnedValueBag>,
    context_values: &UnorderedMap<String, OwnedValueBag>,
    config: &CollectionConfig<'_>,
    provider: &dyn InputProvider,
) -> Result<UnorderedMap<String, OwnedValueBag>, Error> {
    validate_input_configurations(inputs)?;
    let mut values = UnorderedMap::default();

    let mut ctx_vals = omni_tera::Context::new();
    for (key, value) in context_values {
        ctx_vals.insert(key, value);
    }

    for input in inputs {
        let if_expr = input.condition();

        if let Some(if_expr) = if_expr
            && skip(
                if_expr,
                &values,
                &ctx_vals,
                config.if_expressions_root_property,
            )?
        {
            continue;
        }

        let key = input.name().to_string();
        let validators = get_validators(input);
        let pre_exec_value = pre_exec_values.get(&key);

        let value = get_input_value(
            config,
            &ctx_vals,
            input,
            validators,
            &key,
            pre_exec_value,
            provider,
        )
        .await?;

        values.insert(key, value);
    }

    Ok(values)
}

pub async fn collect_one<TExtra: InputExtras>(
    input: &InputConfiguration<TExtra>,
    pre_exec_value: Option<&OwnedValueBag>,
    context_values: &UnorderedMap<String, OwnedValueBag>,
    config: &CollectionConfig<'_>,
    provider: &dyn InputProvider,
) -> Result<Option<OwnedValueBag>, Error> {
    let mut ctx_vals = omni_tera::Context::new();
    for (key, value) in context_values {
        ctx_vals.insert(key, value);
    }

    let if_expr = input.condition();
    let mut pre_exec_values = unordered_map!();
    if let Some(pre_exec_value) = pre_exec_value {
        pre_exec_values
            .insert(input.name().to_string(), pre_exec_value.clone());
    }

    if let Some(if_expr) = if_expr
        && skip(
            if_expr,
            &pre_exec_values,
            &ctx_vals,
            config.if_expressions_root_property,
        )?
    {
        return Ok(None);
    }

    let validators = get_validators(input);
    let key = input.name().to_string();
    let value = get_input_value(
        config,
        &ctx_vals,
        input,
        validators,
        &key,
        pre_exec_value,
        provider,
    )
    .await?;

    Ok(Some(value))
}

async fn get_input_value<TExtra: InputExtras>(
    config: &CollectionConfig<'_>,
    ctx_vals: &omni_tera::Context,
    input: &InputConfiguration<TExtra>,
    validators: &[ValidateConfiguration],
    key: &String,
    pre_exec_value: Option<&OwnedValueBag>,
    provider: &dyn InputProvider,
) -> Result<OwnedValueBag, Error> {
    let value = if let Some(pre_exec_value) = pre_exec_value {
        log::debug!("using pre-exec value for input {key}: {pre_exec_value}");
        process_pre_filled_value(
            config,
            ctx_vals,
            input,
            validators,
            key,
            pre_exec_value,
            false,
            provider,
        )
        .await?
    } else if config.use_defaults
        && let Some(value) = input.default_value()
    {
        log::debug!("using default value for input {key}: {value}");
        process_pre_filled_value(
            config, ctx_vals, input, validators, key, &value, true, provider,
        )
        .await?
    } else {
        log::debug!(
            "no pre-exec or default value for input {key}, collecting from user"
        );
        get_raw_input_value(input, ctx_vals, provider).await?
    };
    Ok(value)
}

async fn process_pre_filled_value<TExtra: InputExtras>(
    config: &CollectionConfig<'_>,
    ctx_vals: &omni_tera::Context,
    input: &InputConfiguration<TExtra>,
    validators: &[ValidateConfiguration],
    key: &String,
    value: &OwnedValueBag,
    expand_str_value: bool,
    provider: &dyn InputProvider,
) -> Result<OwnedValueBag, Error> {
    let value = match input.discriminant() {
        InputType::Confirm => {
            let bool = try_parse_bool(value.by_ref()).ok_or_else(|| {
                make_input_type_error(key, value.by_ref(), "boolean")
            })?;
            ValueBag::capture_serde1(&bool).to_owned()
        }
        InputType::Float => {
            let float = try_parse_float(value.by_ref()).ok_or_else(|| {
                make_input_type_error(key, value.by_ref(), "float")
            })?;
            ValueBag::capture_serde1(&float).to_owned()
        }
        InputType::Integer => {
            let int = try_parse_int(value.by_ref()).ok_or_else(|| {
                make_input_type_error(key, value.by_ref(), "integer")
            })?;
            ValueBag::capture_serde1(&int).to_owned()
        }
        InputType::Select
        | InputType::MultiSelect
        | InputType::Text
        | InputType::Password => value.clone(),
    };

    let mut is_string_convertible = IsStringConvertible::default();
    value.by_ref().visit(&mut is_string_convertible)?;

    let value = if expand_str_value && is_string_convertible.value {
        if let Some(template) = value.by_ref().to_str() {
            let expanded = omni_tera::one_off(
                template,
                format!("default value for {key}"),
                ctx_vals,
            )?;
            ValueBag::from_str(&expanded).to_owned()
        } else {
            log::warn!(
                "Failed to expand default value for input {key} because it's not a string, using the original value: {value:#?}"
            );
            value
        }
    } else {
        value
    };

    let result = validate_value(
        key,
        &value,
        ctx_vals,
        validators,
        config.validation_value_name,
    );
    Ok(if let Err(err) = result {
        if err.kind() == ErrorKind::InvalidValue {
            log::warn!("re-collecting due to validation error: {err}");
            get_raw_input_value(input, ctx_vals, provider).await?
        } else {
            return Err(err);
        }
    } else {
        value.clone()
    })
}

async fn get_raw_input_value<TExtra: InputExtras>(
    input: &InputConfiguration<TExtra>,
    context_values: &omni_tera::Context,
    provider: &dyn InputProvider,
) -> Result<OwnedValueBag, Error> {
    let value = match input {
        InputConfiguration::Confirm { input, .. } => {
            let v = provider.confirm(input, context_values).await?;
            ValueBag::capture_serde1(&v).to_owned()
        }
        InputConfiguration::Select { input, .. } => {
            let v = provider.select(input, context_values).await?;
            ValueBag::capture_serde1(&v).to_owned()
        }
        InputConfiguration::MultiSelect { input, .. } => {
            let v = provider.multi_select(input, context_values).await?;
            ValueBag::capture_serde1(&v).to_owned()
        }
        InputConfiguration::Text { input, .. } => {
            let v = provider.text(input, context_values).await?;
            ValueBag::capture_serde1(&v).to_owned()
        }
        InputConfiguration::Password { input, .. } => {
            let v = provider.password(input, context_values).await?;
            ValueBag::capture_serde1(&v).to_owned()
        }
        InputConfiguration::Float { input, .. } => {
            let v = provider.float_number(input, context_values).await?;
            ValueBag::capture_serde1(&v).to_owned()
        }
        InputConfiguration::Integer { input, .. } => {
            let v = provider.integer_number(input, context_values).await?;
            ValueBag::capture_serde1(&v).to_owned()
        }
    };
    Ok(value)
}

fn get_validators<TExtra: InputExtras>(
    input: &InputConfiguration<TExtra>,
) -> &[ValidateConfiguration] {
    match input {
        InputConfiguration::Confirm { .. } => &[],
        InputConfiguration::Select { .. } => &[],
        InputConfiguration::MultiSelect { .. } => &[],
        InputConfiguration::Text { input, .. } => &input.base.validate,
        InputConfiguration::Password { input, .. } => &input.base.validate,
        InputConfiguration::Float { input, .. } => &input.base.validate,
        InputConfiguration::Integer { input, .. } => &input.base.validate,
    }
}

fn skip(
    if_expr: &Either<bool, String>,
    values: &UnorderedMap<String, OwnedValueBag>,
    context_values: &omni_tera::Context,
    if_expressions_root_property: Option<&str>,
) -> Result<bool, Error> {
    Ok(match if_expr {
        Either::Left(left) => !*left,
        Either::Right(if_expr) => {
            let mut ctx = context_values.clone();
            ctx.insert(
                if_expressions_root_property.unwrap_or("inputs"),
                values,
            );

            let tera_result = omni_tera::one_off(if_expr, if_expr, &ctx)?;
            let tera_result = tera_result.trim();

            validate_boolean_expression_result(&tera_result, if_expr)?;

            tera_result != "true"
        }
    })
}

fn validate_input_configurations<TExtra: InputExtras>(
    inputs: &[InputConfiguration<TExtra>],
) -> Result<(), Error> {
    let mut seen_names = UnorderedSet::default();

    for input in inputs {
        let name = input.name();

        if seen_names.contains(&name) {
            return Err(ErrorInner::DuplicateInputName(name.to_string()))?;
        }

        seen_names.insert(name);
    }

    Ok(())
}

fn try_parse_bool(value: value_bag::ValueBag<'_>) -> Option<bool> {
    if let Some(value) = value.to_bool() {
        return Some(value);
    }
    if let Some(value) = value.to_str() {
        return Some(value.parse::<bool>().ok()?);
    }
    None
}

fn try_parse_float(value: value_bag::ValueBag<'_>) -> Option<f64> {
    if let Some(value) = value.to_f64() {
        return Some(value);
    }
    if let Some(value) = value.to_str() {
        return Some(value.parse::<f64>().ok()?);
    }
    None
}

fn try_parse_int(value: value_bag::ValueBag<'_>) -> Option<i64> {
    if let Some(value) = value.to_i64() {
        return Some(value);
    }
    if let Some(value) = value.to_str() {
        return Some(value.parse::<i64>().ok()?);
    }
    None
}

fn make_input_type_error<'a>(
    input_name: &'a str,
    value: value_bag::ValueBag<'a>,
    expected_type: &'a str,
) -> Error {
    Error::from(eyre::eyre!(
        "{input_name}: value is not of type {expected_type}: value {value}",
        value =
            serde_json::to_string_pretty(&value).expect("should be converted"),
    ))
}

#[derive(Default)]
struct IsStringConvertible {
    pub value: bool,
}

impl<'v> value_bag::visit::Visit<'v> for IsStringConvertible {
    fn visit_borrowed_str(
        &mut self,
        _value: &'v str,
    ) -> Result<(), value_bag::Error> {
        self.value = true;
        Ok(())
    }

    fn visit_str(&mut self, _value: &str) -> Result<(), value_bag::Error> {
        self.value = true;
        Ok(())
    }

    fn visit_any(
        &mut self,
        _value: value_bag::ValueBag,
    ) -> Result<(), value_bag::Error> {
        self.value = false;
        Ok(())
    }
}

// ── Validate ──────────────────────────────────────────────────────────────────

/// A validation error for a single named input field.
#[derive(Debug)]
pub struct ValidationError {
    pub input_name: String,
    pub message: String,
}

/// The outcome of a [`validate`] call.
pub struct ValidationReport {
    pub errors: Vec<ValidationError>,
}

impl ValidationReport {
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Validate a set of pre-supplied input values against an input schema without
/// collecting anything interactively.
///
/// For each active (non-skipped) input:
/// - emits a [`ValidationError`] when the value is missing and no default is
///   available (accounting for `config.use_defaults`)
/// - type-checks numeric / boolean inputs
/// - runs all Tera-based validator expressions via [`validate_value`]
///
/// Infrastructure errors (e.g. a malformed Tera template in an `if` condition)
/// are returned as `Err`.
pub fn validate<TExtra: InputExtras>(
    inputs: &[InputConfiguration<TExtra>],
    input_values: &UnorderedMap<String, OwnedValueBag>,
    context_values: &UnorderedMap<String, OwnedValueBag>,
    config: &CollectionConfig<'_>,
) -> Result<ValidationReport, Error> {
    validate_input_configurations(inputs)?;

    let mut ctx_vals = omni_tera::Context::new();
    for (key, value) in context_values {
        ctx_vals.insert(key, value);
    }

    let mut errors = Vec::new();
    // Tracks the resolved effective value for each processed input so that
    // later condition expressions (e.g. `{{ inputs.version == 'custom' }}`)
    // can reference earlier inputs' values even when they came from defaults.
    let mut effective_values: UnorderedMap<String, OwnedValueBag> =
        input_values.clone();

    for input in inputs {
        let name = input.name();

        if let Some(if_expr) = input.condition()
            && skip(
                if_expr,
                &effective_values,
                &ctx_vals,
                config.if_expressions_root_property,
            )?
        {
            continue;
        }

        let value = input_values.get(name);
        let has_default =
            config.use_defaults && input.default_value().is_some();

        if value.is_none() && !has_default {
            errors.push(ValidationError {
                input_name: name.to_string(),
                message: format!("required input '{name}' is missing"),
            });
            continue;
        }

        // Populate effective_values with the default so subsequent condition
        // expressions see this input's resolved value.
        if value.is_none()
            && has_default
            && let Some(default) = input.default_value()
        {
            effective_values.insert(name.to_string(), default);
        }

        if let Some(value) = value {
            let typed_value = match input.discriminant() {
                InputType::Confirm => try_parse_bool(value.by_ref())
                    .ok_or_else(|| {
                        make_input_type_error(name, value.by_ref(), "boolean")
                    })
                    .map(|b| ValueBag::capture_serde1(&b).to_owned()),
                InputType::Float => try_parse_float(value.by_ref())
                    .ok_or_else(|| {
                        make_input_type_error(name, value.by_ref(), "float")
                    })
                    .map(|f| ValueBag::capture_serde1(&f).to_owned()),
                InputType::Integer => try_parse_int(value.by_ref())
                    .ok_or_else(|| {
                        make_input_type_error(name, value.by_ref(), "integer")
                    })
                    .map(|i| ValueBag::capture_serde1(&i).to_owned()),
                _ => Ok(value.clone()),
            };

            match typed_value {
                Err(e) => errors.push(ValidationError {
                    input_name: name.to_string(),
                    message: e.to_string(),
                }),
                Ok(typed) => {
                    effective_values.insert(name.to_string(), typed.clone());
                    if let Err(e) = validate_value(
                        name,
                        &typed,
                        &ctx_vals,
                        get_validators(input),
                        config.validation_value_name,
                    ) {
                        errors.push(ValidationError {
                            input_name: name.to_string(),
                            message: e.to_string(),
                        });
                    }
                }
            }
        }
    }

    Ok(ValidationReport { errors })
}

#[cfg(test)]
mod tests {
    use either::Either;
    use maps::UnorderedMap;
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use value_bag::{OwnedValueBag, ValueBag};

    use super::{collect, collect_one, validate};
    use crate::{
        configuration::{CollectionConfig, InputConfiguration, builder},
        scripted::ScriptedInputProvider,
    };

    /// Minimal `TExtra` for tests — satisfies `InputExtras` without a full derive.
    #[derive(
        Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema,
    )]
    struct NoExtra;

    impl garde::Validate for NoExtra {
        type Context = ();

        fn validate_into(
            &self,
            _ctx: &Self::Context,
            _parent: &mut dyn FnMut() -> garde::Path,
            _report: &mut garde::Report,
        ) {
        }
    }

    // ── input builders ────────────────────────────────────────────────────────

    fn text(name: &'static str) -> InputConfiguration<NoExtra> {
        builder::text::<NoExtra>().name(name).message(name).build()
    }

    fn text_with_default(
        name: &'static str,
        default: &'static str,
    ) -> InputConfiguration<NoExtra> {
        builder::text::<NoExtra>()
            .name(name)
            .message(name)
            .default(default)
            .build()
    }

    fn text_with_condition(
        name: &'static str,
        condition: Option<Either<bool, String>>,
    ) -> InputConfiguration<NoExtra> {
        builder::text::<NoExtra>()
            .name(name)
            .message(name)
            .maybe_condition(condition.map(builder::ValueOrExpr::from))
            .build()
    }

    fn text_with_validator(
        name: &'static str,
        expr: &'static str,
    ) -> InputConfiguration<NoExtra> {
        builder::text::<NoExtra>()
            .name(name)
            .message(name)
            .validate([(expr, "validation failed")])
            .build()
    }

    fn confirm(name: &'static str) -> InputConfiguration<NoExtra> {
        builder::confirm::<NoExtra>()
            .name(name)
            .message(name)
            .build()
    }

    fn integer(name: &'static str) -> InputConfiguration<NoExtra> {
        builder::integer::<NoExtra>()
            .name(name)
            .message(name)
            .build()
    }

    // ── value / map helpers ───────────────────────────────────────────────────

    fn str_val(s: &str) -> OwnedValueBag {
        ValueBag::from_serde1(&s).to_owned()
    }

    fn bool_val(b: bool) -> OwnedValueBag {
        ValueBag::from_serde1(&b).to_owned()
    }

    fn int_val(i: i64) -> OwnedValueBag {
        ValueBag::from_serde1(&i).to_owned()
    }

    fn one(
        name: &str,
        val: OwnedValueBag,
    ) -> UnorderedMap<String, OwnedValueBag> {
        let mut m = UnorderedMap::default();
        m.insert(name.to_string(), val);
        m
    }

    fn cfg(use_defaults: bool) -> CollectionConfig<'static> {
        CollectionConfig {
            use_defaults,
            ..Default::default()
        }
    }

    fn scripted(answers: &[(&str, &str)]) -> ScriptedInputProvider {
        ScriptedInputProvider::new(answers.iter().copied())
    }

    fn empty() -> UnorderedMap<String, OwnedValueBag> {
        Default::default()
    }

    /// Serialize an `OwnedValueBag` to `serde_json::Value` for assertions.
    fn jv(v: &OwnedValueBag) -> serde_json::Value {
        serde_json::to_value(v).expect("OwnedValueBag must serialize to JSON")
    }

    // ── validate ──────────────────────────────────────────────────────────────

    #[test]
    fn missing_required_field_produces_error() {
        let inputs = [text("name")];
        let report =
            validate(&inputs, &empty(), &empty(), &cfg(false)).unwrap();
        assert!(!report.is_valid());
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].input_name, "name");
    }

    #[test]
    fn provided_required_field_is_valid() {
        let inputs = [text("name")];
        let values = one("name", str_val("Alice"));
        let report = validate(&inputs, &values, &empty(), &cfg(false)).unwrap();
        assert!(report.is_valid());
    }

    #[test]
    fn defaulted_field_not_required_when_use_defaults_true() {
        let inputs = [text_with_default("env", "development")];
        let report = validate(&inputs, &empty(), &empty(), &cfg(true)).unwrap();
        assert!(report.is_valid());
    }

    #[test]
    fn defaulted_field_required_when_use_defaults_false() {
        let inputs = [text_with_default("env", "development")];
        let report =
            validate(&inputs, &empty(), &empty(), &cfg(false)).unwrap();
        assert!(!report.is_valid());
        assert_eq!(report.errors[0].input_name, "env");
    }

    #[test]
    fn always_hidden_field_is_never_required() {
        let inputs = [text_with_condition("secret", Some(Either::Left(false)))];
        let report =
            validate(&inputs, &empty(), &empty(), &cfg(false)).unwrap();
        assert!(report.is_valid());
    }

    #[test]
    fn conditional_field_skipped_when_tera_condition_is_false() {
        let inputs = [
            text("kind"),
            text_with_condition(
                "extra",
                Some(Either::Right(
                    "{{ inputs.kind == 'advanced' }}".to_string(),
                )),
            ),
        ];
        let values = one("kind", str_val("basic"));
        let report = validate(&inputs, &values, &empty(), &cfg(false)).unwrap();
        assert!(report.is_valid());
    }

    #[test]
    fn conditional_field_required_when_tera_condition_is_true() {
        let inputs = [
            text("kind"),
            text_with_condition(
                "extra",
                Some(Either::Right(
                    "{{ inputs.kind == 'advanced' }}".to_string(),
                )),
            ),
        ];
        let values = one("kind", str_val("advanced"));
        let report = validate(&inputs, &values, &empty(), &cfg(false)).unwrap();
        assert!(!report.is_valid());
        assert_eq!(report.errors[0].input_name, "extra");
    }

    #[test]
    fn confirm_rejects_non_bool_string() {
        let inputs = [confirm("enabled")];
        let values = one("enabled", str_val("not-a-bool"));
        let report = validate(&inputs, &values, &empty(), &cfg(false)).unwrap();
        assert!(!report.is_valid());
        assert_eq!(report.errors[0].input_name, "enabled");
    }

    #[test]
    fn confirm_accepts_bool_value() {
        let inputs = [confirm("enabled")];
        let values = one("enabled", bool_val(true));
        let report = validate(&inputs, &values, &empty(), &cfg(false)).unwrap();
        assert!(report.is_valid());
    }

    #[test]
    fn integer_rejects_non_numeric_string() {
        let inputs = [integer("count")];
        let values = one("count", str_val("not-a-number"));
        let report = validate(&inputs, &values, &empty(), &cfg(false)).unwrap();
        assert!(!report.is_valid());
        assert_eq!(report.errors[0].input_name, "count");
    }

    #[test]
    fn integer_accepts_integer_value() {
        let inputs = [integer("count")];
        let values = one("count", int_val(42));
        let report = validate(&inputs, &values, &empty(), &cfg(false)).unwrap();
        assert!(report.is_valid());
    }

    #[test]
    fn validator_expression_rejects_short_value() {
        let inputs = [text_with_validator("name", "{{ value | length > 3 }}")];
        let values = one("name", str_val("ab"));
        let report = validate(&inputs, &values, &empty(), &cfg(false)).unwrap();
        assert!(!report.is_valid());
        assert_eq!(report.errors[0].input_name, "name");
    }

    #[test]
    fn validator_expression_accepts_valid_value() {
        let inputs = [text_with_validator("name", "{{ value | length > 3 }}")];
        let values = one("name", str_val("Alice"));
        let report = validate(&inputs, &values, &empty(), &cfg(false)).unwrap();
        assert!(report.is_valid());
    }

    #[test]
    fn all_errors_collected_not_just_first() {
        let inputs = [text("a"), text("b"), text("c")];
        let report =
            validate(&inputs, &empty(), &empty(), &cfg(false)).unwrap();
        assert_eq!(report.errors.len(), 3);
    }

    #[test]
    fn duplicate_input_names_is_infrastructure_error() {
        let inputs = [text("name"), text("name")];
        let result = validate(&inputs, &empty(), &empty(), &cfg(false));
        assert!(result.is_err());
    }

    // ── collect ───────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn collect_text_via_provider() {
        let inputs = [text("name")];
        let result = collect(
            &inputs,
            &empty(),
            &empty(),
            &cfg(false),
            &scripted(&[("name", "Alice")]),
        )
        .await
        .unwrap();
        assert_eq!(jv(result.get("name").unwrap()), json!("Alice"));
    }

    #[tokio::test]
    async fn collect_pre_exec_value_shadows_provider() {
        // Provider has no answer for "name" — would error if called.
        let inputs = [text("name")];
        let pre_exec = one("name", str_val("Bob"));
        let result =
            collect(&inputs, &pre_exec, &empty(), &cfg(false), &scripted(&[]))
                .await
                .unwrap();
        assert_eq!(jv(result.get("name").unwrap()), json!("Bob"));
    }

    #[tokio::test]
    async fn collect_uses_default_when_use_defaults_true() {
        let inputs = [text_with_default("env", "development")];
        let result =
            collect(&inputs, &empty(), &empty(), &cfg(true), &scripted(&[]))
                .await
                .unwrap();
        assert_eq!(jv(result.get("env").unwrap()), json!("development"));
    }

    #[tokio::test]
    async fn collect_ignores_default_when_use_defaults_false() {
        let inputs = [text_with_default("env", "development")];
        let result = collect(
            &inputs,
            &empty(),
            &empty(),
            &cfg(false),
            &scripted(&[("env", "production")]),
        )
        .await
        .unwrap();
        assert_eq!(jv(result.get("env").unwrap()), json!("production"));
    }

    #[tokio::test]
    async fn collect_skips_always_hidden_field() {
        let inputs = [
            text("visible"),
            text_with_condition("hidden", Some(Either::Left(false))),
        ];
        let result = collect(
            &inputs,
            &empty(),
            &empty(),
            &cfg(false),
            &scripted(&[("visible", "yes")]),
        )
        .await
        .unwrap();
        assert!(result.get("visible").is_some());
        assert!(result.get("hidden").is_none());
    }

    #[tokio::test]
    async fn collect_skips_conditional_field_when_condition_false() {
        // "extra" condition references the already-collected "kind" value.
        let inputs = [
            text("kind"),
            text_with_condition(
                "extra",
                Some(Either::Right(
                    "{{ inputs.kind == 'advanced' }}".to_string(),
                )),
            ),
        ];
        let result = collect(
            &inputs,
            &empty(),
            &empty(),
            &cfg(false),
            &scripted(&[("kind", "basic")]),
        )
        .await
        .unwrap();
        assert_eq!(jv(result.get("kind").unwrap()), json!("basic"));
        assert!(result.get("extra").is_none());
    }

    #[tokio::test]
    async fn collect_includes_conditional_field_when_condition_true() {
        let inputs = [
            text("kind"),
            text_with_condition(
                "extra",
                Some(Either::Right(
                    "{{ inputs.kind == 'advanced' }}".to_string(),
                )),
            ),
        ];
        let result = collect(
            &inputs,
            &empty(),
            &empty(),
            &cfg(false),
            &scripted(&[("kind", "advanced"), ("extra", "bonus")]),
        )
        .await
        .unwrap();
        assert_eq!(jv(result.get("extra").unwrap()), json!("bonus"));
    }

    #[tokio::test]
    async fn collect_coerces_string_pre_exec_to_bool() {
        let inputs = [confirm("enabled")];
        let pre_exec = one("enabled", str_val("true"));
        let result =
            collect(&inputs, &pre_exec, &empty(), &cfg(false), &scripted(&[]))
                .await
                .unwrap();
        assert_eq!(jv(result.get("enabled").unwrap()), json!(true));
    }

    #[tokio::test]
    async fn collect_coerces_string_pre_exec_to_integer() {
        let inputs = [integer("count")];
        let pre_exec = one("count", str_val("42"));
        let result =
            collect(&inputs, &pre_exec, &empty(), &cfg(false), &scripted(&[]))
                .await
                .unwrap();
        assert_eq!(jv(result.get("count").unwrap()), json!(42));
    }

    #[tokio::test]
    async fn collect_falls_back_to_provider_when_pre_exec_fails_validation() {
        // "ab" (length 2) fails the validator; collect() re-asks via provider.
        let inputs = [text_with_validator("name", "{{ value | length > 3 }}")];
        let pre_exec = one("name", str_val("ab"));
        let result = collect(
            &inputs,
            &pre_exec,
            &empty(),
            &cfg(false),
            &scripted(&[("name", "Alice")]),
        )
        .await
        .unwrap();
        assert_eq!(jv(result.get("name").unwrap()), json!("Alice"));
    }

    #[tokio::test]
    async fn collect_expands_default_template_using_context() {
        let inputs = [text_with_default("greeting", "Hello {{ name }}")];
        let context = one("name", str_val("World"));
        let result =
            collect(&inputs, &empty(), &context, &cfg(true), &scripted(&[]))
                .await
                .unwrap();
        assert_eq!(jv(result.get("greeting").unwrap()), json!("Hello World"));
    }

    #[tokio::test]
    async fn collect_all_inputs_present_in_result() {
        let inputs = [text("a"), text("b"), text("c")];
        let result = collect(
            &inputs,
            &empty(),
            &empty(),
            &cfg(false),
            &scripted(&[("a", "1"), ("b", "2"), ("c", "3")]),
        )
        .await
        .unwrap();
        assert_eq!(jv(result.get("a").unwrap()), json!("1"));
        assert_eq!(jv(result.get("b").unwrap()), json!("2"));
        assert_eq!(jv(result.get("c").unwrap()), json!("3"));
    }

    #[tokio::test]
    async fn collect_duplicate_input_names_returns_error() {
        let inputs = [text("x"), text("x")];
        let result = collect(
            &inputs,
            &empty(),
            &empty(),
            &cfg(false),
            &scripted(&[("x", "v")]),
        )
        .await;
        assert!(result.is_err());
    }

    // ── collect_one ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn collect_one_prompts_provider_for_active_input() {
        let input = text("name");
        let result = collect_one(
            &input,
            None,
            &empty(),
            &cfg(false),
            &scripted(&[("name", "Alice")]),
        )
        .await
        .unwrap();
        assert_eq!(jv(result.as_ref().unwrap()), json!("Alice"));
    }

    #[tokio::test]
    async fn collect_one_returns_none_for_always_hidden_input() {
        let input = text_with_condition("secret", Some(Either::Left(false)));
        let result =
            collect_one(&input, None, &empty(), &cfg(false), &scripted(&[]))
                .await
                .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn collect_one_uses_pre_exec_value() {
        let input = text("name");
        let pre = str_val("Bob");
        let result = collect_one(
            &input,
            Some(&pre),
            &empty(),
            &cfg(false),
            &scripted(&[]),
        )
        .await
        .unwrap();
        assert_eq!(jv(result.as_ref().unwrap()), json!("Bob"));
    }

    #[tokio::test]
    async fn collect_one_returns_some_when_condition_is_always_true() {
        let input = text_with_condition("label", Some(Either::Left(true)));
        let result = collect_one(
            &input,
            None,
            &empty(),
            &cfg(false),
            &scripted(&[("label", "visible")]),
        )
        .await
        .unwrap();
        assert!(result.is_some());
        assert_eq!(jv(result.as_ref().unwrap()), json!("visible"));
    }
}
