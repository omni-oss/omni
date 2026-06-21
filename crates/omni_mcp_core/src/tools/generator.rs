use std::path::PathBuf;
use std::sync::Arc;

use maps::UnorderedMap;
use omni_api::{
    ForwardedInputs, GeneratorInputKind, GeneratorInspectResponse,
    GeneratorRunRequest, GeneratorValidateInputRequest, InputCondition,
    InputDefault, StaticInputDefault, SubGeneratorRef,
    SubGeneratorValidationResult,
};
use omni_context::ContextSys;
use omni_generator::GeneratorSys;
use omni_input_provider::{
    ConfirmInputConfiguration, FloatInputConfiguration, InputProvider,
    IntegerInputConfiguration, MultiSelectInputConfiguration,
    PasswordInputConfiguration, SelectInputConfiguration,
    TextInputConfiguration, error::Error as InputError,
};
use omni_messages::OmniEventSubscriber;
use omni_task_executor::TaskExecutorSys;
use value_bag::{OwnedValueBag, ValueBag};

use crate::{
    model::{
        GeneratorInspectParams, GeneratorInspectResult, GeneratorListResult,
        GeneratorRunParams, GeneratorRunResult, GeneratorSummary,
        GeneratorValidateInputParams, GeneratorValidateInputResult,
        McpForwardedInputs, McpInputCondition, McpInputFieldError,
        McpInputOption, McpInputSpec, McpSubGeneratorRef,
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
        let response = self.make_api().generator_inspect(&params.name).await?;
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
            target: vec![],
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
    GeneratorInspectResult {
        name: resp.name,
        display_name: resp.display_name,
        description: resp.description,
        inputs: resp.inputs.into_iter().map(translate_input_spec).collect(),
        targets: resp
            .targets
            .into_iter()
            .map(|t| McpTargetSpec {
                key: t.key,
                default_path: t.default_path,
            })
            .collect(),
        sub_generators: resp
            .sub_generators
            .into_iter()
            .map(translate_sub_generator_ref)
            .collect(),
    }
}

fn translate_input_spec(input: omni_api::GeneratorInputSpec) -> McpInputSpec {
    let kind = match input.kind {
        GeneratorInputKind::Confirm => "confirm",
        GeneratorInputKind::Select => "select",
        GeneratorInputKind::MultiSelect => "multi-select",
        GeneratorInputKind::Text => "text",
        GeneratorInputKind::Password => "password",
        GeneratorInputKind::Float => "float",
        GeneratorInputKind::Integer => "integer",
    }
    .to_string();

    let condition = input.condition.map(|c| match c {
        InputCondition::AlwaysHidden => McpInputCondition::AlwaysHidden,
        InputCondition::Expression { expr } => {
            McpInputCondition::Expression { expr }
        }
    });

    let (default_value, has_dynamic_default) = match input.default {
        None => (None, false),
        Some(InputDefault::Dynamic { .. }) => (None, true),
        Some(InputDefault::Static { value }) => (
            Some(match value {
                StaticInputDefault::Bool(b) => serde_json::Value::Bool(b),
                StaticInputDefault::Int(i) => {
                    serde_json::Value::Number(i.into())
                }
                StaticInputDefault::Float(f) => serde_json::Number::from_f64(f)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null),
                StaticInputDefault::Str(s) => serde_json::Value::String(s),
                StaticInputDefault::StrList(v) => serde_json::Value::Array(
                    v.into_iter().map(serde_json::Value::String).collect(),
                ),
            }),
            false,
        ),
    };

    let required = default_value.is_none()
        && !has_dynamic_default
        && !matches!(&condition, Some(McpInputCondition::AlwaysHidden));

    McpInputSpec {
        name: input.name,
        message: input.message,
        description: input.description,
        kind,
        required,
        default: default_value,
        has_dynamic_default,
        options: input
            .options
            .into_iter()
            .map(|o| McpInputOption {
                label: o.label,
                description: o.description,
                value: o.value,
            })
            .collect(),
        condition,
        validators: input
            .validators
            .into_iter()
            .map(|v| McpValidator {
                condition: v.condition,
                error_message: v.error_message,
            })
            .collect(),
        remember: input.remember,
    }
}

fn translate_sub_generator_ref(r: SubGeneratorRef) -> McpSubGeneratorRef {
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
        generator: r
            .generator
            .map(|g| Box::new(translate_inspect_response(*g))),
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
impl InputProvider for NeverInputProvider {
    async fn confirm(
        &self,
        _input: &ConfirmInputConfiguration,
        _ctx: &omni_tera::Context,
    ) -> Result<bool, InputError> {
        Err(
            eyre::eyre!("NeverInputProvider: interactive input not supported")
                .into(),
        )
    }

    async fn text(
        &self,
        _input: &TextInputConfiguration,
        _ctx: &omni_tera::Context,
    ) -> Result<String, InputError> {
        Err(
            eyre::eyre!("NeverInputProvider: interactive input not supported")
                .into(),
        )
    }

    async fn password(
        &self,
        _input: &PasswordInputConfiguration,
        _ctx: &omni_tera::Context,
    ) -> Result<String, InputError> {
        Err(
            eyre::eyre!("NeverInputProvider: interactive input not supported")
                .into(),
        )
    }

    async fn select(
        &self,
        _input: &SelectInputConfiguration,
        _ctx: &omni_tera::Context,
    ) -> Result<String, InputError> {
        Err(
            eyre::eyre!("NeverInputProvider: interactive input not supported")
                .into(),
        )
    }

    async fn multi_select(
        &self,
        _input: &MultiSelectInputConfiguration,
        _ctx: &omni_tera::Context,
    ) -> Result<Vec<String>, InputError> {
        Err(
            eyre::eyre!("NeverInputProvider: interactive input not supported")
                .into(),
        )
    }

    async fn float_number(
        &self,
        _input: &FloatInputConfiguration,
        _ctx: &omni_tera::Context,
    ) -> Result<f64, InputError> {
        Err(
            eyre::eyre!("NeverInputProvider: interactive input not supported")
                .into(),
        )
    }

    async fn integer_number(
        &self,
        _input: &IntegerInputConfiguration,
        _ctx: &omni_tera::Context,
    ) -> Result<i64, InputError> {
        Err(
            eyre::eyre!("NeverInputProvider: interactive input not supported")
                .into(),
        )
    }
}
