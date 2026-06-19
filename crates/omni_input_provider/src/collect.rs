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
