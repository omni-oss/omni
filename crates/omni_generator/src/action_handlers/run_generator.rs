use std::borrow::Cow;

use maps::{UnorderedMap, unordered_map};
use omni_generator_configurations::{
    ForAllInputValuesConfiguration, ForwardInputValuesConfiguration,
    InputValue, Root, RunGeneratorActionConfiguration,
};
use omni_messages::{
    DiagnosticEvent, DiagnosticLevel, GeneratorEventSubscriber,
};
use value_bag::{OwnedValueBag, ValueBag};

use crate::{
    GeneratorSysFull, RunConfig,
    action_handlers::HandlerContext,
    error::{Error, ErrorInner},
    run_internal,
};

pub async fn run_generator<'a, S: GeneratorEventSubscriber>(
    config: &RunGeneratorActionConfiguration,
    ctx: &HandlerContext<'a, S>,
    sys: &impl GeneratorSysFull,
) -> Result<(), Error> {
    let generator = ctx
        .available_generators
        .iter()
        .find(|g| g.name == config.generator)
        .ok_or_else(|| ErrorInner::GeneratorNotFound {
            name: config.generator.clone(),
        })?;

    let parent_inputs = ctx.context_values
        .get("inputs")
        .expect("should have prompt vaues, if you encountered this error, please report it to the maintainers");

    let input_values = resolve_input_values(parent_inputs, config, &ctx)?;

    ctx.subscriber
        .on_diagnostic(DiagnosticEvent {
            level: DiagnosticLevel::Trace,
            message: format!("resolved prompt values: {input_values:#?}"),
            fields: Default::default(),
            target: "omni::generator::run_generator".to_string(),
        })
        .await;

    let target_overrides = if config.targets.is_empty() {
        Cow::Borrowed(ctx.target_overrides)
    } else {
        let mut map = ctx.target_overrides.clone();

        for (key, value) in &config.targets {
            if !map.contains_key(key) {
                map.insert(key.clone(), value.clone());
            }
        }

        Cow::Owned(map)
    };

    ctx.subscriber
        .on_diagnostic(DiagnosticEvent {
            level: DiagnosticLevel::Trace,
            message: format!(
                "resolved target overrides: {target_overrides:#?}"
            ),
            fields: Default::default(),
            target: "omni::generator::run_generator".to_string(),
        })
        .await;

    let override_output_dir = config.output_dir.as_ref().map(|d| {
        let base = enum_map::enum_map! {
            Root::Output => ctx.output_dir,
            Root::Workspace => ctx.workspace_dir,
        };

        d.resolve(&base)
    });
    let output_dir = if let Some(override_output_dir) = override_output_dir {
        if override_output_dir.is_absolute() {
            override_output_dir
        } else {
            Cow::Owned(ctx.output_dir.join(override_output_dir))
        }
    } else {
        Cow::Borrowed(ctx.output_dir)
    };

    let available_generators = if let Some(a_scope_id) = ctx.scope_id {
        let generators = ctx
            .available_generators
            .iter()
            .filter(|g| {
                if let Some(b_scope_id) = g.scope_id.as_deref() {
                    b_scope_id == a_scope_id
                } else {
                    false
                }
            })
            .cloned()
            .collect::<Vec<_>>();

        Cow::Owned(generators)
    } else {
        Cow::Borrowed(ctx.available_generators)
    };

    let run_config = RunConfig {
        dry_run: ctx.dry_run,
        output_dir: &output_dir,
        workspace_dir: ctx.workspace_dir,
        overwrite: ctx.overwrite,
        context_values: ctx.context_values,
        input_values: input_values.as_ref(),
        target_overrides: target_overrides.as_ref(),
        current_dir: ctx.current_dir,
        env: ctx.env,
        args: Some(&config.args),
        use_input_defaults: ctx.use_input_defaults,
        available_generators: &available_generators,
        input_provider: ctx.input_provider,
        subscriber: ctx.subscriber,
        max_depth: ctx.max_depth,
    };

    let prompted_input_values = Box::pin(run_internal(
        generator,
        &run_config,
        sys,
        ctx.js_script_runner,
        ctx.depth + 1,
    ))
    .await?;

    ctx.gen_session.merge(prompted_input_values).await;

    Ok(())
}

fn resolve_input_values<'a, S: GeneratorEventSubscriber>(
    parent_inputs: &'a OwnedValueBag,
    config: &RunGeneratorActionConfiguration,
    ctx: &HandlerContext<'a, S>,
) -> Result<Cow<'a, UnorderedMap<String, OwnedValueBag>>, Error> {
    let parsed = serde_json::to_value(parent_inputs)?;

    if !parsed.is_object() {
        return Err(ErrorInner::Custom(eyre::eyre!(
            "prompts should be an object, but got: {parsed:?}"
        ))
        .into());
    }

    let inputs = parsed.as_object().expect("should be object at this point");

    let mut all_values = unordered_map!();

    match &config.input_values.forward {
        ForwardInputValuesConfiguration::ForAll(
            ForAllInputValuesConfiguration::All,
        ) => {
            for (key, value) in inputs {
                all_values.insert(
                    key.clone(),
                    ValueBag::capture_serde1(value).to_owned(),
                );
            }
        }
        ForwardInputValuesConfiguration::ForAll(
            ForAllInputValuesConfiguration::None,
        ) => {
            // do nothing
        }
        ForwardInputValuesConfiguration::Selected(l) => {
            for key in l {
                if let Some(value) = inputs.get(key) {
                    all_values.insert(
                        key.clone(),
                        ValueBag::capture_serde1(value).to_owned(),
                    );
                }
            }
        }
    }

    for (key, value) in config.input_values.values.iter() {
        let converted =
            to_owned_value_bag(key, value, ctx.tera_context_values)?;

        all_values.insert(key.clone(), converted);
    }

    Ok(Cow::Owned(all_values))
}

fn to_owned_value_bag(
    key: &str,
    input_value: &InputValue,
    tera_ctx: &omni_tera::Context,
) -> Result<OwnedValueBag, Error> {
    Ok(match input_value {
        InputValue::Integer(i) => ValueBag::capture_serde1(i).to_owned(),
        InputValue::Float(f) => ValueBag::capture_serde1(f).to_owned(),
        InputValue::Boolean(b) => ValueBag::capture_serde1(b).to_owned(),
        InputValue::String(s) => ValueBag::capture_serde1(&omni_tera::one_off(
            &s,
            &format!("inputs.{key}"),
            tera_ctx,
        )?)
        .to_owned(),
        InputValue::List(values) => {
            let mut list = Vec::with_capacity(values.len());

            for (idx, value) in values.iter().enumerate() {
                list.push(to_owned_value_bag(
                    &format!("{key}.{idx}"),
                    value,
                    tera_ctx,
                )?);
            }

            ValueBag::capture_serde1(&list).to_owned()
        }
    })
}

#[cfg(test)]
mod tests {
    use maps::{UnorderedMap, unordered_map};
    use omni_generator_configurations::{
        BaseActionConfiguration, ForAllInputValuesConfiguration,
        ForwardInputValuesConfiguration, InputValue, InputValuesConfiguration,
        RunGeneratorActionConfiguration,
    };
    use value_bag::ValueBag;

    use super::super::test_harness::Fixture;
    use super::{resolve_input_values, to_owned_value_bag};

    fn run_gen_config(
        forward: ForwardInputValuesConfiguration,
        values: UnorderedMap<String, InputValue>,
    ) -> RunGeneratorActionConfiguration {
        RunGeneratorActionConfiguration {
            base: BaseActionConfiguration {
                r#if: None,
                name: None,
                in_progress_message: None,
                success_message: None,
                error_message: None,
            },
            generator: "gen".to_string(),
            input_values: InputValuesConfiguration { forward, values },
            args: UnorderedMap::default(),
            output_dir: None,
            targets: UnorderedMap::default(),
        }
    }

    #[test]
    fn converts_integer() {
        let ctx = omni_tera::Context::new();
        let val =
            to_owned_value_bag("k", &InputValue::Integer(42), &ctx).unwrap();
        let n = serde_json::to_value(&val).unwrap().as_i64().unwrap();
        assert_eq!(n, 42);
    }

    #[test]
    fn converts_float() {
        let ctx = omni_tera::Context::new();
        let val =
            to_owned_value_bag("k", &InputValue::Float(1.5), &ctx).unwrap();
        let f = serde_json::to_value(&val).unwrap().as_f64().unwrap();
        assert_eq!(f, 1.5);
    }

    #[test]
    fn converts_boolean() {
        let ctx = omni_tera::Context::new();
        let val =
            to_owned_value_bag("k", &InputValue::Boolean(true), &ctx).unwrap();
        let b = serde_json::to_value(&val).unwrap().as_bool().unwrap();
        assert!(b);
    }

    #[test]
    fn expands_string_template() {
        let mut ctx = omni_tera::Context::new();
        ctx.insert("x", "hello");
        let val = to_owned_value_bag(
            "k",
            &InputValue::String("{{ x }}".into()),
            &ctx,
        )
        .unwrap();
        let s = val.by_ref().to_str().unwrap().into_owned();
        assert_eq!(s, "hello");
    }

    #[test]
    fn converts_list() {
        let ctx = omni_tera::Context::new();
        let list = InputValue::List(vec![
            InputValue::Integer(1),
            InputValue::String("hi".into()),
        ]);
        let val = to_owned_value_bag("k", &list, &ctx).unwrap();
        let json = serde_json::to_value(&val).unwrap();
        assert_eq!(json, serde_json::json!([1, "hi"]));
    }

    #[test]
    fn forward_all_copies_all_parent_inputs() {
        let fix = Fixture::new();
        let ctx = fix.ctx();
        let map = serde_json::json!({"a": 1_i64, "b": "foo"});
        let parent_inputs = ValueBag::from_serde1(&map).to_owned();
        let config = run_gen_config(
            ForwardInputValuesConfiguration::ForAll(
                ForAllInputValuesConfiguration::All,
            ),
            unordered_map!(),
        );
        let result =
            resolve_input_values(&parent_inputs, &config, &ctx).unwrap();
        assert!(result.contains_key("a"));
        assert!(result.contains_key("b"));
    }

    #[test]
    fn forward_none_produces_empty_result() {
        let fix = Fixture::new();
        let ctx = fix.ctx();
        let map = serde_json::json!({"a": 1_i64});
        let parent_inputs = ValueBag::from_serde1(&map).to_owned();
        let config = run_gen_config(
            ForwardInputValuesConfiguration::ForAll(
                ForAllInputValuesConfiguration::None,
            ),
            unordered_map!(),
        );
        let result =
            resolve_input_values(&parent_inputs, &config, &ctx).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn forward_selected_copies_only_named_keys() {
        let fix = Fixture::new();
        let ctx = fix.ctx();
        let map = serde_json::json!({"a": 1_i64, "b": "foo", "c": 3_i64});
        let parent_inputs = ValueBag::from_serde1(&map).to_owned();
        let config = run_gen_config(
            ForwardInputValuesConfiguration::Selected(vec![
                "a".into(),
                "c".into(),
            ]),
            unordered_map!(),
        );
        let result =
            resolve_input_values(&parent_inputs, &config, &ctx).unwrap();
        assert!(result.contains_key("a"));
        assert!(result.contains_key("c"));
        assert!(!result.contains_key("b"));
    }

    #[test]
    fn forward_selected_skips_missing_keys_without_error() {
        let fix = Fixture::new();
        let ctx = fix.ctx();
        let map = serde_json::json!({"a": 1_i64});
        let parent_inputs = ValueBag::from_serde1(&map).to_owned();
        let config = run_gen_config(
            ForwardInputValuesConfiguration::Selected(vec![
                "a".into(),
                "missing".into(),
            ]),
            unordered_map!(),
        );
        let result = resolve_input_values(&parent_inputs, &config, &ctx);
        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.contains_key("a"));
        assert!(!result.contains_key("missing"));
    }

    #[test]
    fn explicit_values_override_forwarded_values() {
        let fix = Fixture::new();
        let ctx = fix.ctx();
        let map = serde_json::json!({"a": 1_i64});
        let parent_inputs = ValueBag::from_serde1(&map).to_owned();
        let config = run_gen_config(
            ForwardInputValuesConfiguration::ForAll(
                ForAllInputValuesConfiguration::All,
            ),
            unordered_map!("a".to_string() => InputValue::Integer(99)),
        );
        let result =
            resolve_input_values(&parent_inputs, &config, &ctx).unwrap();
        let n = serde_json::to_value(&result["a"])
            .unwrap()
            .as_i64()
            .unwrap();
        assert_eq!(n, 99);
    }
}
