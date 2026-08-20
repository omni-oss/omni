use maps::OrderedMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Which JavaScript runtime to launch a tool's entrypoint with.
///
/// **Only [`Deno`](Self::Deno) is currently fully supported**, and it is the
/// default for tools. Deno is the sole runtime whose sandboxing — filesystem,
/// network, environment, and child-process confinement — is complete on Linux,
/// macOS, and Windows today.
///
/// `node` and `bun` are accepted for compatibility but are **experimental and
/// discouraged**: their confinement is incomplete or absent, so selecting one
/// (or resolving to one via [`Auto`](Self::Auto)) emits a warning. They are
/// omitted from the JSON schema so editors do not suggest them, but the values
/// remain valid if set explicitly.
#[derive(
    Serialize,
    Deserialize,
    JsonSchema,
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
)]
#[serde(rename_all = "kebab-case")]
pub enum ToolJsRuntime {
    /// The Deno runtime — the recommended, fully-confined runtime on all
    /// platforms. The default for tools.
    #[default]
    Deno,
    /// The Node.js runtime. **Experimental and discouraged:** confinement is
    /// incomplete on some platforms, so forcing it emits a warning. Hidden
    /// from the JSON schema; still accepted if set explicitly.
    #[schemars(skip)]
    Node,
    /// The Bun runtime. **Experimental and discouraged:** it has no native
    /// permission model and runs largely unconfined, so forcing it emits a
    /// warning. Hidden from the JSON schema; still accepted if set explicitly.
    #[schemars(skip)]
    Bun,
    /// Auto-detect a supported runtime on `PATH`, preferring Deno.
    #[schemars(skip)]
    Auto,
}

/// How a `type: pipeline` tool surfaces its result.
#[derive(
    Serialize,
    Deserialize,
    JsonSchema,
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
)]
#[serde(rename_all = "kebab-case")]
pub enum PipelineOutput {
    /// Return the last non-skipped step's output (the default).
    #[default]
    Last,
    /// Return an object mapping every step name to its output.
    All,
}

/// A single step in a `type: pipeline` tool.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PipelineStep {
    /// Unique name; referenced by later steps via `from: steps.<name>.output`.
    pub name: String,
    /// The name of the tool this step invokes.
    pub tool: String,
    /// Inputs handed to the invoked tool, by input name.
    #[serde(default)]
    pub inputs: OrderedMap<String, PipelineInputValue>,
    /// Optional Tera boolean condition. A falsy result skips the step and makes
    /// its output resolve to `null` for later references.
    #[serde(default, rename = "if", skip_serializing_if = "Option::is_none")]
    pub r#if: Option<String>,
}

/// A value supplied to a pipeline step's input.
///
/// Two kinds are disambiguated structurally:
///
/// * a structural reference (`{ from: "steps.<name>.output.<path>" }` or
///   `{ from: "inputs.<name>" }`) that preserves the referenced JSON value's
///   type; and
/// * a literal scalar or a Tera-string that is rendered to a string.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum PipelineInputValue {
    /// A structural reference into pipeline inputs or an earlier step's output.
    Ref {
        /// Dotted path, e.g. `steps.fetch.output.files` or `inputs.dir`.
        from: String,
    },
    /// A boolean literal.
    Boolean(bool),
    /// An integer literal.
    Integer(i64),
    /// A floating-point literal.
    Float(f64),
    /// A literal or Tera-templated string rendered to a string.
    String(String),
}

/// The execution backend of a tool, selected by the required `type` field.
///
/// Flattened into [`ToolConfiguration`](crate::ToolConfiguration); `type` has no
/// default and must be one of `js` or `pipeline`.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ToolBackend {
    /// A JavaScript entrypoint whose default export is invoked and whose return
    /// value is captured.
    Js {
        /// The JavaScript runtime to launch. Defaults to `deno`.
        #[serde(default)]
        runtime: ToolJsRuntime,
        /// Path to the runnable JavaScript entrypoint, relative to the manifest.
        entrypoint: String,
    },
    /// A sequence of steps that chain other tools together.
    Pipeline {
        /// Ordered steps executed in declaration order.
        steps: Vec<PipelineStep>,
        /// How the pipeline surfaces its result. Defaults to `last`.
        #[serde(default)]
        output: PipelineOutput,
    },
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn js_backend_defaults_runtime_to_deno() {
        let backend: ToolBackend = serde_json::from_value(json!({
            "type": "js",
            "entrypoint": "./dist/index.mjs"
        }))
        .expect("parses js backend");
        match backend {
            ToolBackend::Js {
                runtime,
                entrypoint,
            } => {
                assert_eq!(runtime, ToolJsRuntime::Deno);
                assert_eq!(entrypoint, "./dist/index.mjs");
            }
            ToolBackend::Pipeline { .. } => panic!("expected js backend"),
        }
    }

    #[test]
    fn pipeline_backend_defaults_output_to_last() {
        let backend: ToolBackend = serde_json::from_value(json!({
            "type": "pipeline",
            "steps": [
                { "name": "fetch", "tool": "fetch-data", "inputs": { "dir": { "from": "inputs.dir" } } }
            ]
        }))
        .expect("parses pipeline backend");
        match backend {
            ToolBackend::Pipeline { steps, output } => {
                assert_eq!(output, PipelineOutput::Last);
                assert_eq!(steps.len(), 1);
                assert_eq!(steps[0].name, "fetch");
                assert_eq!(steps[0].tool, "fetch-data");
            }
            ToolBackend::Js { .. } => panic!("expected pipeline backend"),
        }
    }

    #[test]
    fn pipeline_input_value_disambiguates_ref_and_scalars() {
        let r: PipelineInputValue =
            serde_json::from_value(json!({ "from": "steps.fetch.output" }))
                .unwrap();
        assert!(matches!(r, PipelineInputValue::Ref { .. }));

        let b: PipelineInputValue =
            serde_json::from_value(json!(true)).unwrap();
        assert!(matches!(b, PipelineInputValue::Boolean(true)));

        let i: PipelineInputValue = serde_json::from_value(json!(7)).unwrap();
        assert!(matches!(i, PipelineInputValue::Integer(7)));

        let s: PipelineInputValue =
            serde_json::from_value(json!("{{ inputs.dir }}")).unwrap();
        assert!(matches!(s, PipelineInputValue::String(_)));
    }

    #[test]
    fn missing_type_is_rejected() {
        let res: Result<ToolBackend, _> =
            serde_json::from_value(json!({ "entrypoint": "./x.mjs" }));
        assert!(res.is_err(), "type has no default and is required");
    }
}
