use std::borrow::Borrow;

use maps::{UnorderedMap, unordered_map};
use omni_config_types::MaybeExpr;
use omni_input_schema::{Input, InputProfile, ObjectInput, ValidationConfig};
use sets::UnorderedSet;
use value_bag::{OwnedValueBag, ValueBag};

use crate::{
    error::{Error, ErrorInner, ErrorKind},
    provider::InputProvider,
    utils::{validate_boolean_expression_result, validate_value},
};

/// Interactively collect values for every active input in `inputs`.
///
/// - Pre-exec and default values short-circuit provider calls.
/// - If a pre-filled value fails validation the provider is called to
///   re-ask.
/// - The Tera context is seeded with `context_values`; collected values
///   accumulate in it so later `if` expressions can reference them.
pub async fn collect<E: InputProfile + Send + Sync + 'static>(
    inputs: &[Input<E>],
    pre_exec_values: &UnorderedMap<String, OwnedValueBag>,
    context_values: &UnorderedMap<String, OwnedValueBag>,
    config: &ValidationConfig<'_>,
    provider: &dyn InputProvider<E>,
) -> Result<UnorderedMap<String, OwnedValueBag>, Error> {
    let ctx = to_tera_ctx(context_values);

    let values =
        collect_internal(inputs, pre_exec_values, &ctx, config, provider)
            .await?;

    Ok(values)
}

/// Collect a single input, respecting its `if` condition.
/// Returns `None` when the condition gates the input out.
pub async fn collect_one<E: InputProfile + Send + Sync + 'static>(
    input: &Input<E>,
    pre_exec_value: Option<&OwnedValueBag>,
    context_values: &UnorderedMap<String, OwnedValueBag>,
    config: &ValidationConfig<'_>,
    provider: &dyn InputProvider<E>,
) -> Result<Option<OwnedValueBag>, Error> {
    let ctx = to_tera_ctx(context_values);

    let pre_exec_values = match pre_exec_value {
        Some(v) => unordered_map! { input.base().name.clone() => v.clone() },
        None => unordered_map!(),
    };

    let mut values =
        collect_internal(&[input], &pre_exec_values, &ctx, config, provider)
            .await?;

    Ok(values.remove(&input.base().name))
}

const MAX_RETRIES: usize = 5;

// ── Internals ─────────────────────────────────────────────────────────────────
//

async fn collect_internal<
    E: InputProfile + Send + Sync + 'static,
    I: Borrow<Input<E>>,
>(
    inputs: &[I],
    pre_exec_values: &UnorderedMap<String, OwnedValueBag>,
    ctx: &omni_tera::Context,
    config: &ValidationConfig<'_>,
    provider: &dyn InputProvider<E>,
) -> Result<UnorderedMap<String, OwnedValueBag>, Error> {
    check_no_duplicate_names(inputs)?;

    let mut values = UnorderedMap::default();

    for input in inputs {
        let base = input.borrow().base();

        if let Some(if_expr) = &base.r#if {
            if skip(if_expr, &values, ctx, config.if_expressions_root_property)?
            {
                continue;
            }
        }

        let key = base.name.clone();
        let pre = pre_exec_values.get(&key);

        let value =
            get_input_value(config, ctx, input, &key, pre, provider).await?;
        values.insert(key, value);
    }

    Ok(values)
}

fn to_tera_ctx(
    context_values: &UnorderedMap<String, OwnedValueBag>,
) -> omni_tera::Context {
    let mut ctx = omni_tera::Context::new();
    for (k, v) in context_values {
        ctx.insert(k.to_owned(), v);
    }

    ctx
}

async fn get_input_value<
    E: InputProfile + Send + Sync + 'static,
    I: Borrow<Input<E>>,
>(
    config: &ValidationConfig<'_>,
    ctx: &omni_tera::Context,
    input: &I,
    key: &String,
    pre_exec_value: Option<&OwnedValueBag>,
    provider: &dyn InputProvider<E>,
) -> Result<OwnedValueBag, Error> {
    let input = input.borrow();
    if let Some(v) = pre_exec_value {
        log::debug!("using pre-exec value for input {key}: {v}");
        process_pre_filled_value(config, ctx, input, key, v, false, provider)
            .await
    } else if config.use_defaults
        && let Some(expr) = input.dynamic_default_expr()
    {
        log::debug!(
            "evaluating dynamic default expression for input {key}: {expr}"
        );
        let expanded = omni_tera::one_off(
            expr,
            format!("dynamic default for {key}"),
            ctx,
        )?;
        let rendered = ValueBag::from_str(&expanded).to_owned();
        process_pre_filled_value(
            config, ctx, input, key, &rendered, false, provider,
        )
        .await
    } else if config.use_defaults
        && let Some(default) = input.static_default_value_bag()
    {
        log::debug!("using default value for input {key}: {default}");
        process_pre_filled_value(
            config, ctx, input, key, &default, true, provider,
        )
        .await
    } else {
        log::debug!("collecting input {key} from provider");
        get_raw_input_value(input, ctx, provider, config, MAX_RETRIES).await
    }
}

async fn process_pre_filled_value<
    E: InputProfile + Send + Sync + 'static,
    I: Borrow<Input<E>>,
>(
    config: &ValidationConfig<'_>,
    ctx: &omni_tera::Context,
    input: &I,
    key: &String,
    value: &OwnedValueBag,
    expand_str: bool,
    provider: &dyn InputProvider<E>,
) -> Result<OwnedValueBag, Error> {
    let input = input.borrow();
    // Coerce the raw value to the correct Rust type (e.g. "true" → bool).
    let value = match input {
        Input::Boolean(_) => try_parse_bool(value.by_ref())
            .ok_or_else(|| make_type_error(key, value.by_ref(), "boolean"))
            .map(|b| ValueBag::capture_serde1(&b).to_owned())?,
        Input::Integer(_) => try_parse_int(value.by_ref())
            .ok_or_else(|| make_type_error(key, value.by_ref(), "integer"))
            .map(|i| ValueBag::capture_serde1(&i).to_owned())?,
        Input::Float(_) => try_parse_float(value.by_ref())
            .ok_or_else(|| make_type_error(key, value.by_ref(), "float"))
            .map(|f| ValueBag::capture_serde1(&f).to_owned())?,
        _ => value.clone(),
    };

    // Expand Tera templates in default string values.
    let value = 'expand: {
        if !expand_str {
            break 'expand value;
        }
        let mut is_str = IsStringConvertible::default();
        value.by_ref().visit(&mut is_str)?;
        if !is_str.value {
            break 'expand value;
        }
        let Some(tmpl) = value.by_ref().to_str() else {
            log::warn!(
                "failed to expand default for {key}: not a string; \
                 using original value"
            );
            break 'expand value;
        };
        let expanded =
            omni_tera::one_off(tmpl, format!("default value for {key}"), ctx)?;
        ValueBag::from_str(&expanded).to_owned()
    };

    // Run validators; re-ask provider on InvalidValue.
    let validators = input.base().validators.as_slice();
    let result = validate_value(
        key,
        &value,
        ctx,
        validators,
        config.validation_value_name,
    );
    match result {
        Ok(()) => Ok(value),
        Err(e) if e.kind() == ErrorKind::InvalidValue => {
            get_raw_input_value(input, ctx, provider, config, MAX_RETRIES).await
        }
        Err(e) => Err(e),
    }
}

async fn get_raw_input_value<
    E: InputProfile + Send + Sync + 'static,
    I: Borrow<Input<E>>,
>(
    input: &I,
    ctx: &omni_tera::Context,
    provider: &dyn InputProvider<E>,
    config: &ValidationConfig<'_>,
    max_retries: usize,
) -> Result<OwnedValueBag, Error> {
    let mut tries = 0;
    let input = input.borrow();
    loop {
        let result = match input {
            Input::Boolean(b) => {
                ValueBag::capture_serde1(&provider.boolean(b, ctx).await?)
                    .to_owned()
            }
            Input::String(s) => {
                ValueBag::from_serde1(&provider.string(s, ctx).await?)
                    .to_owned()
            }
            Input::Integer(i) => {
                ValueBag::capture_serde1(&provider.integer(i, ctx).await?)
                    .to_owned()
            }
            Input::Float(f) => {
                ValueBag::capture_serde1(&provider.float(f, ctx).await?)
                    .to_owned()
            }
            Input::StringArray(sa) => {
                ValueBag::from_serde1(&provider.string_array(sa, ctx).await?)
                    .to_owned()
            }
            Input::IntegerArray(ia) => {
                ValueBag::from_serde1(&provider.integer_array(ia, ctx).await?)
                    .to_owned()
            }
            Input::FloatArray(fa) => {
                ValueBag::from_serde1(&provider.float_array(fa, ctx).await?)
                    .to_owned()
            }
            Input::Object(a) if provider.supports_native_object_input() => {
                ValueBag::from_serde1(&provider.object(a, ctx).await?)
                    .to_owned()
            }
            Input::Object(o) => {
                collect_from_object(o, ctx, config, provider).await?
            }
        };

        let validators = input.base().validators.as_slice();
        let validation_result = validate_value(
            &input.base().name,
            &result,
            ctx,
            validators,
            config.validation_value_name,
        );

        if let Err(err) = validation_result {
            tries += 1;
            if tries >= max_retries {
                return Err(err);
            }
            log::warn!(
                "re-collecting input {} due to validation error: {}",
                input.base().name,
                err
            );
            continue;
        }

        break Ok(result);
    }
}

async fn collect_from_object<E: InputProfile + Send + Sync + 'static>(
    object_input: &ObjectInput<E>,
    context: &omni_tera::Context,
    config: &ValidationConfig<'_>,
    provider: &dyn InputProvider<E>,
) -> Result<OwnedValueBag, Error> {
    let object = Box::pin(collect_internal(
        object_input.fields.as_slice(),
        &unordered_map!(),
        context,
        config,
        provider,
    ))
    .await?;

    Ok(ValueBag::from_serde1(&object).to_owned())
}

fn skip(
    if_expr: &MaybeExpr<bool>,
    values: &UnorderedMap<String, OwnedValueBag>,
    ctx: &omni_tera::Context,
    root_property: Option<&str>,
) -> Result<bool, Error> {
    Ok(match if_expr {
        MaybeExpr::Value(b) => !*b,
        MaybeExpr::Expr(expr) => {
            let mut eval_ctx = ctx.clone();
            eval_ctx
                .insert(root_property.unwrap_or("inputs").to_owned(), values);
            let result = omni_tera::one_off(expr, expr, &eval_ctx)?;
            let result = result.trim();
            validate_boolean_expression_result(result, expr)?;
            result != "true"
        }
    })
}

fn check_no_duplicate_names<E: InputProfile, I: Borrow<Input<E>>>(
    inputs: &[I],
) -> Result<(), Error> {
    let mut seen = UnorderedSet::default();
    for input in inputs {
        let name = input.borrow().base().name.as_str();
        if seen.contains(name) {
            return Err(ErrorInner::DuplicateInputName(name.to_string()))?;
        }
        seen.insert(name);
    }
    Ok(())
}

fn try_parse_bool(value: value_bag::ValueBag<'_>) -> Option<bool> {
    value.to_bool().or_else(|| value.to_str()?.parse().ok())
}

fn try_parse_float(value: value_bag::ValueBag<'_>) -> Option<f64> {
    value.to_f64().or_else(|| value.to_str()?.parse().ok())
}

fn try_parse_int(value: value_bag::ValueBag<'_>) -> Option<i64> {
    value.to_i64().or_else(|| value.to_str()?.parse().ok())
}

fn make_type_error(
    input_name: &str,
    value: value_bag::ValueBag<'_>,
    expected_type: &str,
) -> Error {
    Error::from(eyre::eyre!(
        "{input_name}: value is not of type {expected_type}: value {}",
        serde_json::to_string_pretty(&value).expect("should serialize"),
    ))
}

#[derive(Default)]
struct IsStringConvertible {
    value: bool,
}

impl<'v> value_bag::visit::Visit<'v> for IsStringConvertible {
    fn visit_borrowed_str(
        &mut self,
        _: &'v str,
    ) -> Result<(), value_bag::Error> {
        self.value = true;
        Ok(())
    }
    fn visit_str(&mut self, _: &str) -> Result<(), value_bag::Error> {
        self.value = true;
        Ok(())
    }
    fn visit_any(
        &mut self,
        _: value_bag::ValueBag,
    ) -> Result<(), value_bag::Error> {
        self.value = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use maps::UnorderedMap;
    use omni_input_schema::{Input, ValidationConfig};
    use serde_json::json;
    use value_bag::{OwnedValueBag, ValueBag};

    use super::{collect, collect_one};
    use crate::scripted::ScriptedInputProvider;

    // ── helpers ───────────────────────────────────────────────────────────────

    fn parse<T: for<'de> serde::Deserialize<'de>>(v: serde_json::Value) -> T {
        serde_json::from_value(v).expect("parse failed")
    }

    fn bool_input(name: &str) -> Input<()> {
        parse(json!({"type": "boolean", "name": name}))
    }
    fn str_input(name: &str) -> Input<()> {
        parse(json!({"type": "string", "name": name}))
    }
    fn str_input_with_default(name: &str, default: &str) -> Input<()> {
        parse(json!({"type": "string", "name": name, "default": default}))
    }
    fn str_input_with_condition(
        name: &str,
        condition: serde_json::Value,
    ) -> Input<()> {
        parse(json!({"type": "string", "name": name, "if": condition}))
    }
    fn str_input_with_validator(name: &str, expr: &str) -> Input<()> {
        parse(json!({"type": "string", "name": name,
            "validators": [{"condition": expr}]}))
    }
    fn int_input(name: &str) -> Input<()> {
        parse(json!({"type": "integer", "name": name}))
    }
    fn float_input(name: &str) -> Input<()> {
        parse(json!({"type": "float", "name": name}))
    }
    fn str_array_input(name: &str) -> Input<()> {
        parse(json!({"type": "string-array", "name": name}))
    }
    fn int_array_input(name: &str) -> Input<()> {
        parse(json!({"type": "integer-array", "name": name}))
    }
    fn float_array_input(name: &str) -> Input<()> {
        parse(json!({"type": "float-array", "name": name}))
    }

    fn str_val(s: &str) -> OwnedValueBag {
        ValueBag::from_serde1(&s.to_string()).to_owned()
    }
    fn one(
        name: &str,
        val: OwnedValueBag,
    ) -> UnorderedMap<String, OwnedValueBag> {
        let mut m = UnorderedMap::default();
        m.insert(name.to_string(), val);
        m
    }
    fn empty() -> UnorderedMap<String, OwnedValueBag> {
        UnorderedMap::default()
    }
    fn cfg(use_defaults: bool) -> ValidationConfig<'static> {
        ValidationConfig {
            use_defaults,
            ..Default::default()
        }
    }
    fn scripted(answers: &[(&str, &str)]) -> ScriptedInputProvider {
        ScriptedInputProvider::new(answers.iter().copied())
    }
    fn jv(v: &OwnedValueBag) -> serde_json::Value {
        serde_json::to_value(v).unwrap()
    }

    // ── collect ───────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn collect_string_via_provider() {
        let result = collect(
            &[str_input("name")],
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
    async fn collect_pre_exec_shadows_provider() {
        let result = collect(
            &[str_input("name")],
            &one("name", str_val("Bob")),
            &empty(),
            &cfg(false),
            &scripted(&[]),
        )
        .await
        .unwrap();
        assert_eq!(jv(result.get("name").unwrap()), json!("Bob"));
    }

    #[tokio::test]
    async fn collect_uses_default_when_use_defaults_true() {
        let result = collect(
            &[str_input_with_default("env", "dev")],
            &empty(),
            &empty(),
            &cfg(true),
            &scripted(&[]),
        )
        .await
        .unwrap();
        assert_eq!(jv(result.get("env").unwrap()), json!("dev"));
    }

    #[tokio::test]
    async fn collect_ignores_default_when_use_defaults_false() {
        let result = collect(
            &[str_input_with_default("env", "dev")],
            &empty(),
            &empty(),
            &cfg(false),
            &scripted(&[("env", "prod")]),
        )
        .await
        .unwrap();
        assert_eq!(jv(result.get("env").unwrap()), json!("prod"));
    }

    #[tokio::test]
    async fn collect_skips_always_hidden() {
        let result = collect(
            &[
                str_input("visible"),
                str_input_with_condition("hidden", json!(false)),
            ],
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
    async fn collect_skips_conditional_when_false() {
        let inputs = [
            str_input("kind"),
            str_input_with_condition(
                "extra",
                json!("{{ inputs.kind == 'advanced' }}"),
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
        assert!(result.get("extra").is_none());
    }

    #[tokio::test]
    async fn collect_includes_conditional_when_true() {
        let inputs = [
            str_input("kind"),
            str_input_with_condition(
                "extra",
                json!("{{ inputs.kind == 'advanced' }}"),
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
        let result = collect(
            &[bool_input("flag")],
            &one("flag", str_val("true")),
            &empty(),
            &cfg(false),
            &scripted(&[]),
        )
        .await
        .unwrap();
        assert_eq!(jv(result.get("flag").unwrap()), json!(true));
    }

    #[tokio::test]
    async fn collect_coerces_string_pre_exec_to_integer() {
        let result = collect(
            &[int_input("count")],
            &one("count", str_val("42")),
            &empty(),
            &cfg(false),
            &scripted(&[]),
        )
        .await
        .unwrap();
        assert_eq!(jv(result.get("count").unwrap()), json!(42));
    }

    #[tokio::test]
    async fn collect_re_asks_provider_when_pre_exec_fails_validation() {
        let result = collect(
            &[str_input_with_validator("name", "{{ value | length > 3 }}")],
            &one("name", str_val("ab")),
            &empty(),
            &cfg(false),
            &scripted(&[("name", "Alice")]),
        )
        .await
        .unwrap();
        assert_eq!(jv(result.get("name").unwrap()), json!("Alice"));
    }

    #[tokio::test]
    async fn collect_expands_default_template() {
        let result = collect(
            &[str_input_with_default("greeting", "Hello {{ name }}")],
            &empty(),
            &one("name", str_val("World")),
            &cfg(true),
            &scripted(&[]),
        )
        .await
        .unwrap();
        assert_eq!(jv(result.get("greeting").unwrap()), json!("Hello World"));
    }

    #[tokio::test]
    async fn collect_all_inputs_present() {
        let result = collect(
            &[str_input("a"), str_input("b"), str_input("c")],
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
    async fn collect_duplicate_names_returns_error() {
        let result = collect(
            &[str_input("x"), str_input("x")],
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
    async fn collect_one_prompts_provider() {
        let result = collect_one(
            &str_input("name"),
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
    async fn collect_one_returns_none_for_hidden() {
        let result = collect_one(
            &str_input_with_condition("x", json!(false)),
            None,
            &empty(),
            &cfg(false),
            &scripted(&[]),
        )
        .await
        .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn collect_one_uses_pre_exec() {
        let pre = str_val("Bob");
        let result = collect_one(
            &str_input("name"),
            Some(&pre),
            &empty(),
            &cfg(false),
            &scripted(&[]),
        )
        .await
        .unwrap();
        assert_eq!(jv(result.as_ref().unwrap()), json!("Bob"));
    }

    // ── variant routing (all 7 interactive variants) ──────────────────────────

    #[tokio::test]
    async fn collect_dispatches_float_variant() {
        let result = collect(
            &[float_input("rate")],
            &empty(),
            &empty(),
            &cfg(false),
            &scripted(&[("rate", "3.14")]),
        )
        .await
        .unwrap();
        assert_eq!(jv(result.get("rate").unwrap()), json!(3.14));
    }

    #[tokio::test]
    async fn collect_dispatches_string_array_variant() {
        let result = collect(
            &[str_array_input("tags")],
            &empty(),
            &empty(),
            &cfg(false),
            &scripted(&[("tags", "a, b, c")]),
        )
        .await
        .unwrap();
        assert_eq!(jv(result.get("tags").unwrap()), json!(["a", "b", "c"]));
    }

    #[tokio::test]
    async fn collect_dispatches_integer_array_variant() {
        let result = collect(
            &[int_array_input("ids")],
            &empty(),
            &empty(),
            &cfg(false),
            &scripted(&[("ids", "1, 2, 3")]),
        )
        .await
        .unwrap();
        assert_eq!(jv(result.get("ids").unwrap()), json!([1, 2, 3]));
    }

    #[tokio::test]
    async fn collect_dispatches_float_array_variant() {
        let result = collect(
            &[float_array_input("vals")],
            &empty(),
            &empty(),
            &cfg(false),
            &scripted(&[("vals", "1.1, 2.2")]),
        )
        .await
        .unwrap();
        assert_eq!(jv(result.get("vals").unwrap()), json!([1.1, 2.2]));
    }

    // ── pre-filled value covers all coercible variants ────────────────────────

    #[tokio::test]
    async fn collect_pre_exec_shadows_provider_for_float() {
        let result = collect(
            &[float_input("score")],
            &one("score", ValueBag::from_serde1(&2.5f64).to_owned()),
            &empty(),
            &cfg(false),
            &scripted(&[]),
        )
        .await
        .unwrap();
        assert_eq!(jv(result.get("score").unwrap()), json!(2.5));
    }

    #[tokio::test]
    async fn collect_coerces_string_pre_exec_to_float() {
        let result = collect(
            &[float_input("score")],
            &one("score", str_val("1.5")),
            &empty(),
            &cfg(false),
            &scripted(&[]),
        )
        .await
        .unwrap();
        assert_eq!(jv(result.get("score").unwrap()), json!(1.5));
    }

    #[tokio::test]
    async fn collect_uses_default_when_use_defaults_true_integer() {
        let input: Input<()> = serde_json::from_value(
            json!({"type": "integer", "name": "count", "default": 7}),
        )
        .unwrap();
        let result =
            collect(&[input], &empty(), &empty(), &cfg(true), &scripted(&[]))
                .await
                .unwrap();
        assert_eq!(jv(result.get("count").unwrap()), json!(7));
    }

    // ── dynamic_default_expr tests ────────────────────────────────────────────
    // A minimal InputProfile that carries `default_expr` in its base extras.
    // Defined locally to avoid a circular dev-dependency: omni_generator_configurations
    // already depends on omni_input_provider.

    #[derive(
        Debug,
        Clone,
        Copy,
        Default,
        PartialEq,
        serde::Serialize,
        serde::Deserialize,
    )]
    struct DynProfile;

    #[derive(
        Debug,
        Clone,
        Default,
        PartialEq,
        serde::Serialize,
        serde::Deserialize,
        schemars::JsonSchema,
    )]
    struct DynBase {
        #[serde(default)]
        default_expr: Option<String>,
    }

    impl omni_input_schema::InputProfile for DynProfile {
        type Base = DynBase;
        type Boolean = ();
        type String = ();
        type Numeric = ();
        type Array = ();
        type Object = ();
        type AllowedValueBase = ();
    }

    #[tokio::test]
    async fn collect_uses_dynamic_default_expr_for_boolean() {
        let input: Input<DynProfile> = serde_json::from_value(json!({
            "type": "boolean",
            "name": "flag",
            "default": "{{ mode == 'prod' }}"
        }))
        .unwrap();

        let mut ctx_values = UnorderedMap::default();
        ctx_values
            .insert("mode".to_string(), ValueBag::from_str("prod").to_owned());

        let result = collect(
            std::slice::from_ref(&input),
            &empty(),
            &ctx_values,
            &cfg(true),
            &scripted(&[]),
        )
        .await
        .unwrap();

        // "prod" == "prod" -> true
        let val = result.get("flag").unwrap();
        assert_eq!(val.by_ref().to_bool(), Some(true));
    }

    #[tokio::test]
    async fn collect_uses_dynamic_default_expr_for_integer() {
        let input: Input<DynProfile> = serde_json::from_value(json!({
            "type": "integer",
            "name": "port",
            "default": "{{ base_port }}"
        }))
        .unwrap();

        let mut ctx_values = UnorderedMap::default();
        ctx_values.insert(
            "base_port".to_string(),
            ValueBag::from_str("8080").to_owned(),
        );

        let result = collect(
            std::slice::from_ref(&input),
            &empty(),
            &ctx_values,
            &cfg(true),
            &scripted(&[]),
        )
        .await
        .unwrap();

        let val = result.get("port").unwrap();
        assert_eq!(val.by_ref().to_i64(), Some(8080));
    }

    // ── object input helpers ─────────────────────────────────────────────────

    fn object_input_with_fields(
        name: &str,
        fields: serde_json::Value,
    ) -> Input<()> {
        parse(json!({"type": "object", "name": name, "fields": fields}))
    }

    /// Provider that handles `Object` inputs natively by returning a
    /// pre-configured JSON value, and delegates every scalar / array variant
    /// to an inner [`ScriptedInputProvider`].
    ///
    /// Used to test the **native** object path in `get_raw_input_value`:
    ///
    /// ```text
    /// Input::Object(a) if provider.supports_native_object_input() =>
    ///     provider.object(a, ctx).await
    /// ```
    #[derive(Debug)]
    struct NativeObjectProvider {
        scripted: ScriptedInputProvider,
        objects: std::collections::HashMap<String, serde_json::Value>,
    }

    impl NativeObjectProvider {
        fn new(
            scalar_answers: &[(&str, &str)],
            objects: &[(&str, serde_json::Value)],
        ) -> Self {
            Self {
                scripted: ScriptedInputProvider::new(
                    scalar_answers.iter().copied(),
                ),
                objects: objects
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.clone()))
                    .collect(),
            }
        }
    }

    #[async_trait::async_trait]
    impl crate::provider::InputProvider<()> for NativeObjectProvider {
        async fn boolean(
            &self,
            input: &omni_input_schema::BooleanInput<()>,
            ctx: &omni_tera::Context,
        ) -> Result<bool, crate::error::Error> {
            self.scripted.boolean(input, ctx).await
        }

        async fn string(
            &self,
            input: &omni_input_schema::StringInput<()>,
            ctx: &omni_tera::Context,
        ) -> Result<String, crate::error::Error> {
            self.scripted.string(input, ctx).await
        }

        async fn integer(
            &self,
            input: &omni_input_schema::IntegerInput<()>,
            ctx: &omni_tera::Context,
        ) -> Result<i64, crate::error::Error> {
            self.scripted.integer(input, ctx).await
        }

        async fn float(
            &self,
            input: &omni_input_schema::FloatInput<()>,
            ctx: &omni_tera::Context,
        ) -> Result<f64, crate::error::Error> {
            self.scripted.float(input, ctx).await
        }

        async fn string_array(
            &self,
            input: &omni_input_schema::StringArrayInput<()>,
            ctx: &omni_tera::Context,
        ) -> Result<Vec<String>, crate::error::Error> {
            self.scripted.string_array(input, ctx).await
        }

        async fn integer_array(
            &self,
            input: &omni_input_schema::IntegerArrayInput<()>,
            ctx: &omni_tera::Context,
        ) -> Result<Vec<i64>, crate::error::Error> {
            self.scripted.integer_array(input, ctx).await
        }

        async fn float_array(
            &self,
            input: &omni_input_schema::FloatArrayInput<()>,
            ctx: &omni_tera::Context,
        ) -> Result<Vec<f64>, crate::error::Error> {
            self.scripted.float_array(input, ctx).await
        }

        fn supports_native_object_input(&self) -> bool {
            true
        }

        async fn object(
            &self,
            input: &omni_input_schema::ObjectInput<()>,
            _ctx: &omni_tera::Context,
        ) -> Result<serde_json::Value, crate::error::Error> {
            self.objects.get(&input.base.name).cloned().ok_or_else(|| {
                crate::error::Error::from(eyre::eyre!(
                    "NativeObjectProvider: no object registered for '{}'",
                    input.base.name
                ))
            })
        }
    }

    #[tokio::test]
    async fn collect_ignores_dynamic_default_when_use_defaults_false() {
        let input: Input<DynProfile> = serde_json::from_value(json!({
            "type": "integer",
            "name": "port",
            "default_expr": "{{ base_port }}"
        }))
        .unwrap();

        let mut ctx_values = UnorderedMap::default();
        ctx_values.insert(
            "base_port".to_string(),
            ValueBag::from_str("9999").to_owned(),
        );

        // use_defaults=false => dynamic default is skipped; provider is called.
        let result = collect(
            std::slice::from_ref(&input),
            &empty(),
            &ctx_values,
            &cfg(false),
            &scripted(&[("port", "1234")]),
        )
        .await
        .unwrap();

        // Provider returned 1234, not the context value 9999.
        let val = result.get("port").unwrap();
        assert_eq!(val.by_ref().to_i64(), Some(1234));
    }

    // ── emulated object path ──────────────────────────────────────────────────
    //
    // ScriptedInputProvider.supports_native_object_input() == false, so
    // collect_from_object() is called and each declared field is collected
    // individually via the provider.

    #[tokio::test]
    async fn collect_emulated_object_collects_fields() {
        // Flat object: two fields are collected field-by-field from the
        // scripted provider and reassembled into a JSON object.
        let input = object_input_with_fields(
            "db",
            json!([
                {"type": "string",  "name": "host"},
                {"type": "integer", "name": "port"},
            ]),
        );
        let result = collect(
            &[input],
            &empty(),
            &empty(),
            &cfg(false),
            &scripted(&[("host", "localhost"), ("port", "5432")]),
        )
        .await
        .unwrap();
        assert_eq!(
            jv(result.get("db").unwrap()),
            json!({"host": "localhost", "port": 5432}),
        );
    }

    #[tokio::test]
    async fn collect_emulated_object_uses_top_level_default() {
        // When use_defaults=true and the object carries a `default` map,
        // that map is used as-is; field-level collection is skipped entirely.
        let input: Input<()> = parse(json!({
            "type": "object",
            "name": "config",
            "fields": [{"type": "string", "name": "env"}],
            "default": {"env": "production"}
        }));
        let result = collect(
            &[input],
            &empty(),
            &empty(),
            &cfg(true),
            // No scripted answers — proves field collection is bypassed.
            &scripted(&[]),
        )
        .await
        .unwrap();
        assert_eq!(
            jv(result.get("config").unwrap()),
            json!({"env": "production"}),
        );
    }

    #[tokio::test]
    async fn collect_emulated_object_field_skipped_by_condition() {
        // A field with `"if": false` must be absent from the collected object;
        // only the unconditional field should appear.
        let input = object_input_with_fields(
            "opts",
            json!([
                {"type": "string", "name": "required"},
                {"type": "string", "name": "optional", "if": false},
            ]),
        );
        let result = collect(
            &[input],
            &empty(),
            &empty(),
            &cfg(false),
            &scripted(&[("required", "yes")]),
        )
        .await
        .unwrap();
        let obj = jv(result.get("opts").unwrap());
        assert_eq!(obj.get("required"), Some(&json!("yes")));
        assert!(
            obj.get("optional").is_none(),
            "conditional field must be absent from the emulated object"
        );
    }

    #[tokio::test]
    async fn collect_emulated_nested_object_recurses() {
        // An Object field inside an outer Object triggers a recursive call to
        // collect_from_object, collecting each level's fields independently.
        let input = object_input_with_fields(
            "outer",
            json!([
                {"type": "string", "name": "label"},
                {
                    "type": "object",
                    "name": "inner",
                    "fields": [{"type": "integer", "name": "count"}]
                },
            ]),
        );
        let result = collect(
            &[input],
            &empty(),
            &empty(),
            &cfg(false),
            &scripted(&[("label", "top"), ("count", "7")]),
        )
        .await
        .unwrap();
        assert_eq!(
            jv(result.get("outer").unwrap()),
            json!({"label": "top", "inner": {"count": 7}}),
        );
    }

    // ── native object path ────────────────────────────────────────────────────
    //
    // NativeObjectProvider.supports_native_object_input() == true, so
    // provider.object() is called directly; field-level collection is skipped.

    #[tokio::test]
    async fn collect_native_object_delegates_to_provider_method() {
        // The JSON returned by provider.object() is used verbatim, including
        // keys that are not declared in `fields`.  No scalar answers are
        // supplied — if field collection were attempted it would error.
        let input = object_input_with_fields(
            "cfg",
            json!([{"type": "string", "name": "key"}]),
        );
        let provider = NativeObjectProvider::new(
            &[], // no scalar answers
            &[("cfg", json!({"key": "native-value", "extra": true}))],
        );
        let result =
            collect(&[input], &empty(), &empty(), &cfg(false), &provider)
                .await
                .unwrap();
        assert_eq!(
            jv(result.get("cfg").unwrap()),
            json!({"key": "native-value", "extra": true}),
        );
    }

    #[tokio::test]
    async fn collect_native_object_alongside_scalar_inputs() {
        // Scalar inputs still go through the normal provider path while the
        // Object input is handled natively in the same collect call.
        let inputs = [
            str_input("name"),
            object_input_with_fields(
                "meta",
                json!([{"type": "boolean", "name": "active"}]),
            ),
        ];
        let provider = NativeObjectProvider::new(
            &[("name", "Alice")],
            &[("meta", json!({"active": true, "role": "admin"}))],
        );
        let result =
            collect(&inputs, &empty(), &empty(), &cfg(false), &provider)
                .await
                .unwrap();
        assert_eq!(jv(result.get("name").unwrap()), json!("Alice"));
        assert_eq!(
            jv(result.get("meta").unwrap()),
            json!({"active": true, "role": "admin"}),
        );
    }

    #[tokio::test]
    async fn collect_native_object_error_propagates() {
        // When the native provider's object() returns an error, collect must
        // surface it rather than silently falling back to field collection.
        let input = object_input_with_fields(
            "missing",
            json!([{"type": "string", "name": "x"}]),
        );
        // No object registered → provider.object() returns an error.
        let provider = NativeObjectProvider::new(&[], &[]);
        let result =
            collect(&[input], &empty(), &empty(), &cfg(false), &provider).await;
        assert!(
            result.is_err(),
            "expected an error when native provider has no answer for the object"
        );
    }

    // ── pre-exec bypasses both native and emulated paths ──────────────────────

    #[tokio::test]
    async fn collect_object_pre_exec_bypasses_collection() {
        // A pre-exec value for an Object key is used as-is: neither
        // provider.object() (native) nor collect_from_object() (emulated)
        // is invoked.  Verified by supplying no scripted answers.
        let input = object_input_with_fields(
            "settings",
            json!([{"type": "string", "name": "theme"}]),
        );
        let pre = ValueBag::from_serde1(&json!({"theme": "dark"})).to_owned();
        let result = collect(
            &[input],
            &one("settings", pre),
            &empty(),
            &cfg(false),
            &scripted(&[]),
        )
        .await
        .unwrap();
        assert_eq!(
            jv(result.get("settings").unwrap()),
            json!({"theme": "dark"}),
        );
    }
}
