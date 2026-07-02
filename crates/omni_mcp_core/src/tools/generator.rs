use std::path::PathBuf;
use std::sync::Arc;

use maps::{UnorderedMap, unordered_map};
use omni_api::{
    ForwardedInputs, GeneratorInspectNode, GeneratorInspectResponse,
    GeneratorRunRequest, GeneratorValidateInputRequest, InspectViewKind,
    SubGeneratorRef, SubGeneratorValidationResult,
};
use omni_config_types::MaybeExpr;
use omni_context::ContextSys;
use omni_generator::GeneratorSys;
use omni_generator_configurations::Generator;
use omni_input_provider::{
    BooleanInput, FloatArrayInput, FloatInput, Input, InputKind, InputProvider,
    InputSchema, IntegerArrayInput, IntegerInput, StringArrayInput,
    StringInput, error::Error as InputError,
};
use omni_messages::OmniEventSubscriber;
use omni_task_executor::TaskExecutorSys;
use value_bag::{OwnedValueBag, ValueBag};

use crate::{
    model::{
        GeneratorInspectParams, GeneratorInspectResult, GeneratorListResult,
        GeneratorRunParams, GeneratorRunResult, GeneratorSummary,
        GeneratorValidateInputParams, GeneratorValidateInputResult,
        McpAllowedValue, McpForwardedInputs, McpInputCondition,
        McpInputFieldError, McpInputSpec, McpSubGeneratorRef,
        McpSubGeneratorValidationResult, McpTargetSpec, McpValidator,
    },
    server::OmniMcpServer,
};

impl<TSys, S> OmniMcpServer<TSys, S>
where
    TSys: ContextSys
        + GeneratorSys
        + TaskExecutorSys
        + Clone
        + Send
        + Sync
        + 'static,
    S: OmniEventSubscriber + Send + Sync + 'static,
{
    pub(crate) async fn tool_generator_list(
        &self,
    ) -> eyre::Result<GeneratorListResult> {
        let response = self.make_api().generator_list().await?;
        Ok(GeneratorListResult {
            generators: response
                .generators
                .into_iter()
                .map(|g| GeneratorSummary {
                    name: g.name,
                    display_name: g.display_name,
                    description: g.description,
                })
                .collect(),
        })
    }

    pub(crate) async fn tool_generator_inspect(
        &self,
        params: GeneratorInspectParams,
    ) -> eyre::Result<GeneratorInspectResult> {
        let response = self
            .make_api()
            .generator_inspect(&params.name, InspectViewKind::Data)
            .await?;
        Ok(translate_inspect_response(response))
    }

    pub(crate) async fn tool_generator_run(
        &self,
        params: GeneratorRunParams,
    ) -> eyre::Result<GeneratorRunResult> {
        let req = GeneratorRunRequest {
            name: Some(params.name),
            output_dir: PathBuf::from(&params.output_dir),
            project: params.project,
            target: unordered_map!(),
            dry_run: params.dry_run,
            overwrite: None,
            save_session: Some(params.save_session),
            ignore_session: Some(params.ignore_session),
            input_values: deserialize_input_values(params.input_values),
            use_defaults: params.use_defaults,
            input_provider: Arc::new(NeverInputProvider),
            max_depth: params.max_depth,
        };
        let response = self.make_api().generator_run(req).await?;
        Ok(GeneratorRunResult {
            ok: true,
            session_saved: response.session_saved,
            actions: response.actions,
        })
    }

    pub(crate) async fn tool_generator_validate_input(
        &self,
        params: GeneratorValidateInputParams,
    ) -> eyre::Result<GeneratorValidateInputResult> {
        let req = GeneratorValidateInputRequest {
            name: params.name,
            input_values: deserialize_input_values(params.input_values),
            use_defaults: params.use_defaults,
        };
        let response = self.make_api().generator_validate_input(req).await?;
        Ok(GeneratorValidateInputResult {
            valid: response.valid,
            errors: translate_field_errors(response.errors),
            sub_generators: response
                .sub_generators
                .into_iter()
                .map(translate_sub_validation)
                .collect(),
        })
    }
}

fn translate_inspect_response(
    resp: GeneratorInspectResponse,
) -> GeneratorInspectResult {
    let node = match resp {
        GeneratorInspectResponse::Data(node) => node,
        GeneratorInspectResponse::Widget(_) => {
            unreachable!("tool_generator_inspect always uses Data view")
        }
    };
    translate_inspect_node(node)
}

fn translate_inspect_node(
    node: GeneratorInspectNode<Vec<InputSchema>>,
) -> GeneratorInspectResult {
    GeneratorInspectResult {
        name: node.name,
        display_name: node.display_name,
        description: node.description,
        inputs: node.inputs.into_iter().map(input_schema_to_mcp).collect(),
        targets: node
            .targets
            .into_iter()
            .map(|t| McpTargetSpec {
                key: t.key,
                default_path: t.default_path,
            })
            .collect(),
        sub_generators: node
            .sub_generators
            .into_iter()
            .map(translate_sub_generator_ref)
            .collect(),
    }
}

fn input_schema_to_mcp(input: InputSchema) -> McpInputSpec {
    let base = input.base();
    let name = base.name.clone();
    let description = base.description.clone();
    let secret = base.secret;
    let condition = condition_from_if(base.r#if.as_ref());
    let validators = validators_from_base(base);

    let kind = match input.kind() {
        InputKind::Boolean => "boolean",
        InputKind::String => "string",
        InputKind::Integer => "integer",
        InputKind::Float => "float",
        InputKind::StringArray => "string-array",
        InputKind::IntegerArray => "integer-array",
        InputKind::FloatArray => "float-array",
        InputKind::Object => "object",
    }
    .to_string();

    let (default, allowed) = match &input {
        Input::Boolean(b) => (
            b.default
                .as_ref()
                .and_then(MaybeExpr::try_as_value_ref)
                .map(|b| serde_json::Value::Bool(*b)),
            vec![],
        ),
        Input::String(s) => (
            s.default
                .as_ref()
                .map(|v| serde_json::Value::String(v.clone())),
            s.allowed.as_ref().map_or_else(Vec::new, |v| {
                v.iter()
                    .map(|a| McpAllowedValue {
                        value: serde_json::Value::String(a.value.clone()),
                        description: a.description.clone(),
                    })
                    .collect()
            }),
        ),
        Input::Integer(i) => (
            i.default
                .as_ref()
                .and_then(MaybeExpr::try_as_value_ref)
                .map(|v| serde_json::Value::Number((*v).into())),
            i.allowed.as_ref().map_or_else(Vec::new, |v| {
                v.iter()
                    .map(|a| McpAllowedValue {
                        value: serde_json::Value::Number(a.value.into()),
                        description: a.description.clone(),
                    })
                    .collect()
            }),
        ),
        Input::Float(f) => (
            f.default
                .as_ref()
                .and_then(MaybeExpr::try_as_value_ref)
                .and_then(|f| serde_json::Number::from_f64(*f))
                .map(serde_json::Value::Number),
            f.allowed.as_ref().map_or_else(Vec::new, |v| {
                v.iter()
                    .map(|a| McpAllowedValue {
                        value: serde_json::Number::from_f64(a.value)
                            .map(serde_json::Value::Number)
                            .unwrap_or(serde_json::Value::Null),
                        description: a.description.clone(),
                    })
                    .collect()
            }),
        ),
        Input::StringArray(sa) => (
            sa.default.as_ref().map(|v| {
                serde_json::Value::Array(
                    v.iter()
                        .map(|s| serde_json::Value::String(s.clone()))
                        .collect(),
                )
            }),
            sa.body.allowed.as_ref().map_or_else(Vec::new, |v| {
                v.iter()
                    .map(|a| McpAllowedValue {
                        value: serde_json::Value::String(a.value.clone()),
                        description: a.description.clone(),
                    })
                    .collect()
            }),
        ),
        Input::IntegerArray(ia) => (
            ia.default.as_ref().map(|v| {
                serde_json::Value::Array(
                    v.iter()
                        .map(|i| serde_json::Value::Number((*i).into()))
                        .collect(),
                )
            }),
            ia.body.allowed.as_ref().map_or_else(Vec::new, |v| {
                v.iter()
                    .map(|a| McpAllowedValue {
                        value: serde_json::Value::Number(a.value.into()),
                        description: a.description.clone(),
                    })
                    .collect()
            }),
        ),
        Input::FloatArray(fa) => (
            fa.default.as_ref().map(|v| {
                serde_json::Value::Array(
                    v.iter()
                        .filter_map(|f| serde_json::Number::from_f64(*f))
                        .map(serde_json::Value::Number)
                        .collect(),
                )
            }),
            fa.body.allowed.as_ref().map_or_else(Vec::new, |v| {
                v.iter()
                    .map(|a| McpAllowedValue {
                        value: serde_json::Number::from_f64(a.value)
                            .map(serde_json::Value::Number)
                            .unwrap_or(serde_json::Value::Null),
                        description: a.description.clone(),
                    })
                    .collect()
            }),
        ),
        Input::Object(_) => (None, vec![]),
    };

    let required = default.is_none()
        && !matches!(&condition, Some(McpInputCondition::AlwaysHidden));

    McpInputSpec {
        name,
        kind,
        required,
        default,
        has_dynamic_default: false,
        secret,
        allowed,
        condition,
        validators,
        description,
    }
}

fn condition_from_if(
    if_expr: Option<&MaybeExpr<bool>>,
) -> Option<McpInputCondition> {
    match if_expr? {
        MaybeExpr::Value(true) => None,
        MaybeExpr::Value(false) => Some(McpInputCondition::AlwaysHidden),
        MaybeExpr::Expr(expr) => Some(McpInputCondition::Expression {
            expr: expr.to_string(),
        }),
    }
}

fn validators_from_base(
    base: &omni_input_provider::BaseInput,
) -> Vec<McpValidator> {
    base.validators
        .iter()
        .map(|v| McpValidator {
            condition: match &v.condition {
                MaybeExpr::Value(b) => b.to_string(),
                MaybeExpr::Expr(expr) => expr.to_string(),
            },
            error_message: v.error_message.clone(),
        })
        .collect()
}

fn translate_sub_generator_ref(
    r: SubGeneratorRef<Vec<InputSchema>>,
) -> McpSubGeneratorRef {
    let forwarded_inputs = match r.forwarded_inputs {
        ForwardedInputs::All => McpForwardedInputs::All,
        ForwardedInputs::None => McpForwardedInputs::None,
        ForwardedInputs::Selected { names } => {
            McpForwardedInputs::Selected { names }
        }
    };

    let pre_filled_inputs = serde_json::Value::Object(
        r.pre_filled_inputs
            .into_iter()
            .collect::<serde_json::Map<_, _>>(),
    );

    McpSubGeneratorRef {
        name: r.name,
        action_condition: r.action_condition,
        forwarded_inputs,
        pre_filled_inputs,
        generator: r.generator.map(|g| Box::new(translate_inspect_node(*g))),
    }
}

fn deserialize_input_values(
    v: serde_json::Value,
) -> UnorderedMap<String, OwnedValueBag> {
    match v {
        serde_json::Value::Object(map) => map
            .into_iter()
            .map(|(k, v)| (k, ValueBag::from_serde1(&v).to_owned()))
            .collect(),
        _ => Default::default(),
    }
}

fn translate_field_errors(
    errors: Vec<omni_api::InputFieldError>,
) -> Vec<McpInputFieldError> {
    errors
        .into_iter()
        .map(|e| McpInputFieldError {
            input_name: e.input_name,
            message: e.message,
        })
        .collect()
}

fn translate_sub_validation(
    r: SubGeneratorValidationResult,
) -> McpSubGeneratorValidationResult {
    McpSubGeneratorValidationResult {
        generator_name: r.generator_name,
        action_condition: r.action_condition,
        valid: r.valid,
        errors: translate_field_errors(r.errors),
        sub_generators: r
            .sub_generators
            .into_iter()
            .map(translate_sub_validation)
            .collect(),
    }
}

#[derive(Debug)]
struct NeverInputProvider;

#[async_trait::async_trait]
impl InputProvider<Generator> for NeverInputProvider {
    async fn boolean(
        &self,
        _input: &BooleanInput<Generator>,
        _ctx: &omni_tera::Context,
    ) -> Result<bool, InputError> {
        Err(
            eyre::eyre!("NeverInputProvider: interactive input not supported")
                .into(),
        )
    }
    async fn string(
        &self,
        _input: &StringInput<Generator>,
        _ctx: &omni_tera::Context,
    ) -> Result<String, InputError> {
        Err(
            eyre::eyre!("NeverInputProvider: interactive input not supported")
                .into(),
        )
    }
    async fn integer(
        &self,
        _input: &IntegerInput<Generator>,
        _ctx: &omni_tera::Context,
    ) -> Result<i64, InputError> {
        Err(
            eyre::eyre!("NeverInputProvider: interactive input not supported")
                .into(),
        )
    }
    async fn float(
        &self,
        _input: &FloatInput<Generator>,
        _ctx: &omni_tera::Context,
    ) -> Result<f64, InputError> {
        Err(
            eyre::eyre!("NeverInputProvider: interactive input not supported")
                .into(),
        )
    }
    async fn string_array(
        &self,
        _input: &StringArrayInput<Generator>,
        _ctx: &omni_tera::Context,
    ) -> Result<Vec<String>, InputError> {
        Err(
            eyre::eyre!("NeverInputProvider: interactive input not supported")
                .into(),
        )
    }
    async fn integer_array(
        &self,
        _input: &IntegerArrayInput<Generator>,
        _ctx: &omni_tera::Context,
    ) -> Result<Vec<i64>, InputError> {
        Err(
            eyre::eyre!("NeverInputProvider: interactive input not supported")
                .into(),
        )
    }
    async fn float_array(
        &self,
        _input: &FloatArrayInput<Generator>,
        _ctx: &omni_tera::Context,
    ) -> Result<Vec<f64>, InputError> {
        Err(
            eyre::eyre!("NeverInputProvider: interactive input not supported")
                .into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omni_input_provider::{BaseInput, BooleanInput, InputSchema};

    fn make_base(name: &str) -> BaseInput {
        serde_json::from_str(&format!(r#"{{"name":"{}"}}"#, name)).unwrap()
    }

    fn value<T>(v: T) -> MaybeExpr<T> {
        MaybeExpr::Value(v)
    }

    #[test]
    fn boolean_input_uses_data_kind() {
        let input = InputSchema::Boolean(BooleanInput {
            base: make_base("flag"),
            default: Some(value(true)),
            base_extra: (),
            boolean_extra: (),
        });
        let spec = input_schema_to_mcp(input);
        assert_eq!(spec.kind, "boolean");
        assert!(!spec.required); // has default
        assert_eq!(spec.default, Some(serde_json::Value::Bool(true)));
        assert!(spec.allowed.is_empty());
    }

    #[test]
    fn string_input_with_allowed_uses_data_kind() {
        let input: InputSchema = serde_json::from_str(
            r#"{"type":"string","name":"env","allowed":["dev","prod"]}"#,
        )
        .unwrap();
        let spec = input_schema_to_mcp(input);
        assert_eq!(spec.kind, "string");
        assert_eq!(spec.allowed.len(), 2);
        assert_eq!(
            spec.allowed[0].value,
            serde_json::Value::String("dev".into())
        );
    }

    #[test]
    fn secret_field_forwarded() {
        let input: InputSchema = serde_json::from_str(
            r#"{"type":"string","name":"token","secret":true}"#,
        )
        .unwrap();
        let spec = input_schema_to_mcp(input);
        assert!(spec.secret);
        assert_eq!(spec.kind, "string");
    }
}
