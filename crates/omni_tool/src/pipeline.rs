//! The `type: pipeline` backend: chaining other tools, routing one tool's
//! output into the next tool's inputs without string round-trips.

use std::borrow::Cow;

use omni_tera::Context as TeraContext;
use omni_tool_configurations::{
    PipelineInputValue, PipelineOutput, PipelineStep, ToolConfiguration,
};
use serde_json::{Map, Value};

use crate::{
    ToolEnforcement, ToolRunner,
    error::{Error, ErrorInner},
    run::run_named_at_depth,
};

/// Execute a pipeline's `steps` sequentially, routing each step's output into
/// the Tera/`from:` context available to later steps, then surface the result
/// per `output`.
///
/// The first step failure aborts the whole pipeline (there is no per-step retry
/// or recovery). A step whose `if` condition is falsy is skipped and its output
/// resolves to `null` for later references.
///
/// v2 refinements (not yet implemented): a `from:` reference to a later or
/// undeclared step should be a static manifest-validation error rather than a
/// silent `null`; and when a skipped step feeds a required downstream input,
/// validation should fail naming both the skipped step and the consuming
/// step/input. Today both resolve to `null` at run time.
pub(crate) async fn run_pipeline<'a, R: ToolRunner>(
    tools: &'a [Cow<'a, ToolConfiguration>],
    steps: &'a [PipelineStep],
    output: PipelineOutput,
    inputs: Value,
    runner: &'a R,
    enforcement: &'a ToolEnforcement,
    depth: usize,
) -> Result<Value, Error> {
    let mut step_outputs: Map<String, Value> = Map::new();
    let mut last_output: Value = Value::Null;

    for step in steps {
        let root = build_root(&inputs, &step_outputs);
        let tera_ctx = TeraContext::from_serialize(&root)?;

        if let Some(expr) = step.r#if.as_deref() {
            if !eval_if(&step.name, expr, &tera_ctx)? {
                // A skipped step's output resolves to `null` for later refs.
                step_outputs.insert(step.name.clone(), Value::Null);
                continue;
            }
        }

        let step_inputs = resolve_step_inputs(step, &root, &tera_ctx)?;
        let out = run_named_at_depth(
            tools,
            &step.tool,
            Value::Object(step_inputs),
            runner,
            enforcement,
            depth + 1,
        )
        .await?;

        step_outputs.insert(step.name.clone(), out.clone());
        last_output = out;
    }

    match output {
        PipelineOutput::Last => Ok(last_output),
        PipelineOutput::All => Ok(Value::Object(step_outputs)),
    }
}

/// Build the reference root a step resolves `from:`/Tera against:
/// `{ "inputs": <pipeline inputs>, "steps": { "<name>": { "output": <value> } } }`.
fn build_root(inputs: &Value, step_outputs: &Map<String, Value>) -> Value {
    let steps: Map<String, Value> = step_outputs
        .iter()
        .map(|(name, out)| {
            let mut entry = Map::new();
            entry.insert("output".to_string(), out.clone());
            (name.clone(), Value::Object(entry))
        })
        .collect();

    let mut root = Map::new();
    root.insert("inputs".to_string(), inputs.clone());
    root.insert("steps".to_string(), Value::Object(steps));
    Value::Object(root)
}

/// Walk a dotted `from:` path over the reference root, preserving the referenced
/// value's JSON type. A missing segment resolves to `null`.
///
/// v1 walks object keys only; numeric segments indexing into arrays are a v2
/// refinement — a numeric segment against a non-object resolves to `null` today.
fn resolve_from(root: &Value, path: &str) -> Value {
    let mut current = root;
    for segment in path.split('.') {
        match current {
            Value::Object(map) => match map.get(segment) {
                Some(value) => current = value,
                None => return Value::Null,
            },
            _ => return Value::Null,
        }
    }
    current.clone()
}

/// Resolve a step's declared inputs into a concrete JSON object handed to the
/// invoked tool. Structural references preserve JSON types; literal scalars pass
/// through; string values are Tera-rendered against the reference context.
fn resolve_step_inputs(
    step: &PipelineStep,
    root: &Value,
    tera_ctx: &TeraContext,
) -> Result<Map<String, Value>, Error> {
    let mut resolved = Map::new();
    for (name, value) in step.inputs.iter() {
        let out = match value {
            PipelineInputValue::Ref { from } => resolve_from(root, from),
            PipelineInputValue::Boolean(b) => Value::Bool(*b),
            PipelineInputValue::Integer(i) => Value::from(*i),
            PipelineInputValue::Float(f) => Value::from(*f),
            PipelineInputValue::String(template) => {
                Value::String(omni_tera::one_off(
                    template,
                    &format!("pipeline step '{}' input '{name}'", step.name),
                    tera_ctx,
                )?)
            }
        };
        resolved.insert(name.clone(), out);
    }
    Ok(resolved)
}

/// Evaluate a step's `if` condition (a Tera expression that must render to
/// exactly `true` or `false`).
fn eval_if(
    step_name: &str,
    expr: &str,
    tera_ctx: &TeraContext,
) -> Result<bool, Error> {
    let rendered = omni_tera::one_off(
        expr,
        &format!("if condition for pipeline step '{step_name}'"),
        tera_ctx,
    )?;
    match rendered.trim() {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(ErrorInner::InvalidIfCondition {
            step: step_name.to_string(),
            expr: expr.to_string(),
            result: other.to_string(),
        }
        .into()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn resolve_from_walks_nested_objects() {
        let root = json!({
            "inputs": { "dir": "/data" },
            "steps": { "fetch": { "output": { "files": ["a", "b"], "count": 2 } } }
        });
        assert_eq!(resolve_from(&root, "inputs.dir"), json!("/data"));
        assert_eq!(
            resolve_from(&root, "steps.fetch.output.files"),
            json!(["a", "b"])
        );
        assert_eq!(resolve_from(&root, "steps.fetch.output.count"), json!(2));
    }

    #[test]
    fn resolve_from_missing_segment_is_null() {
        let root = json!({ "steps": { "fetch": { "output": {} } } });
        assert_eq!(resolve_from(&root, "steps.nope.output"), Value::Null);
        assert_eq!(
            resolve_from(&root, "steps.fetch.output.missing"),
            Value::Null
        );
        // A numeric segment (v2 array indexing) resolves to null in v1.
        assert_eq!(resolve_from(&root, "inputs.0"), Value::Null);
    }

    #[test]
    fn build_root_nests_step_outputs_under_output() {
        let mut outputs = Map::new();
        outputs.insert("fetch".to_string(), json!({ "n": 1 }));
        let root = build_root(&json!({ "x": 1 }), &outputs);
        assert_eq!(root["inputs"], json!({ "x": 1 }));
        assert_eq!(root["steps"]["fetch"]["output"], json!({ "n": 1 }));
    }

    #[test]
    fn eval_if_true_false_and_error() {
        let ctx = TeraContext::from_serialize(&json!({
            "steps": { "fetch": { "output": { "count": 3 } } }
        }))
        .unwrap();
        assert!(
            eval_if("s", "{{ steps.fetch.output.count > 0 }}", &ctx).unwrap()
        );
        assert!(
            !eval_if("s", "{{ steps.fetch.output.count > 5 }}", &ctx).unwrap()
        );
        assert!(eval_if("s", "not-a-bool", &ctx).is_err());
    }

    #[test]
    fn resolve_step_inputs_mixes_refs_literals_and_templates() {
        let root = json!({
            "inputs": { "dir": "/data" },
            "steps": { "fetch": { "output": { "files": ["a"], "count": 1 } } }
        });
        let ctx = TeraContext::from_serialize(&root).unwrap();
        let step = PipelineStep {
            name: "report".to_string(),
            tool: "summarize".to_string(),
            inputs: [
                (
                    "files".to_string(),
                    PipelineInputValue::Ref {
                        from: "steps.fetch.output.files".to_string(),
                    },
                ),
                ("verbose".to_string(), PipelineInputValue::Boolean(true)),
                (
                    "label".to_string(),
                    PipelineInputValue::String(
                        "dir={{ inputs.dir }}".to_string(),
                    ),
                ),
            ]
            .into_iter()
            .collect(),
            r#if: None,
        };
        let resolved = resolve_step_inputs(&step, &root, &ctx).unwrap();
        assert_eq!(resolved["files"], json!(["a"]));
        assert_eq!(resolved["verbose"], json!(true));
        assert_eq!(resolved["label"], json!("dir=/data"));
    }
}
