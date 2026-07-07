//! Thin `wasm-bindgen` front-end over the pure [`omni_workspace_gen`] core.
//!
//! Contains no generation logic — only marshaling. JS consumers speak
//! camelCase, while the core serializes its natural snake_case, so this crate
//! owns the translation layer (recursive object-key case conversion) in both
//! directions. The core stays wasm-free; this crate is the sole `cdylib`.

use heck::{ToLowerCamelCase, ToSnakeCase};
use omni_workspace_gen::{
    HarnessConfig, OmniRenderOptions, WorkspaceModel, build_model, render_omni,
};
use serde::Serialize;
use serde_json::Value;
use wasm_bindgen::prelude::*;

/// Build a [`WorkspaceModel`] from a (camelCase) harness config.
///
/// Unknown fields (e.g. task-bench-only `logLines`/`tools`/`versions`) are
/// ignored, so the JS harness can pass its full config straight through.
#[wasm_bindgen(js_name = buildModel)]
pub fn build_model_js(config: JsValue) -> Result<JsValue, JsError> {
    console_error_panic_hook::set_once();
    let config: Value =
        serde_wasm_bindgen::from_value(config).map_err(to_js_error)?;
    let model = build_model_value(config).map_err(to_js_error)?;
    to_js(&model)
}

/// Render the omni-layer files for a (camelCase) model + options. Returns an
/// array of `[relativePath, contents]` pairs.
#[wasm_bindgen(js_name = renderOmni)]
pub fn render_omni_js(
    model: JsValue,
    options: JsValue,
) -> Result<JsValue, JsError> {
    console_error_panic_hook::set_once();
    let model: Value =
        serde_wasm_bindgen::from_value(model).map_err(to_js_error)?;
    let options: Value =
        serde_wasm_bindgen::from_value(options).map_err(to_js_error)?;
    let files = render_omni_value(model, options).map_err(to_js_error)?;
    to_js(&files)
}

/// Schema version of the model payload this build emits.
#[wasm_bindgen(js_name = modelVersion)]
pub fn model_version() -> u32 {
    omni_workspace_gen::MODEL_VERSION
}

fn build_model_value(config: Value) -> Result<Value, serde_json::Error> {
    let config: HarnessConfig =
        serde_json::from_value(keys_to_snake_case(config))?;
    let model = build_model(&config);
    Ok(keys_to_camel_case(serde_json::to_value(&model)?))
}

fn render_omni_value(
    model: Value,
    options: Value,
) -> Result<Value, serde_json::Error> {
    let model: WorkspaceModel =
        serde_json::from_value(keys_to_snake_case(model))?;
    let options: OmniRenderOptions =
        serde_json::from_value(keys_to_snake_case(options))?;
    serde_json::to_value(render_omni(&model, &options))
}

fn convert_keys(value: Value, convert: fn(&str) -> String) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| (convert(&k), convert_keys(v, convert)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|v| convert_keys(v, convert))
                .collect(),
        ),
        other => other,
    }
}

fn keys_to_snake_case(value: Value) -> Value {
    convert_keys(value, |k| k.to_snake_case())
}

fn keys_to_camel_case(value: Value) -> Value {
    convert_keys(value, |k| k.to_lower_camel_case())
}

fn to_js(value: &Value) -> Result<JsValue, JsError> {
    // json_compatible serializes maps as plain JS objects (not ES `Map`s).
    value
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(to_js_error)
}

fn to_js_error(error: impl std::fmt::Display) -> JsError {
    JsError::new(&error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn build_model_translates_snake_and_camel() {
        let out = build_model_value(json!({
            "projects": 3,
            "tasksPerProject": 2,
            "dependency": { "strategy": "chain" }
        }))
        .unwrap();

        assert_eq!(out["modelVersion"], 1);
        assert_eq!(out["projects"][0]["dir"], "packages/p-0");
        assert_eq!(out["config"]["tasksPerProject"], 2);
        assert_eq!(out["config"]["projectNameTemplate"], "p-{project_id}");
        // Camel on the way out; no snake_case keys leak through.
        assert!(out.get("model_version").is_none());
        assert!(out["config"].get("tasks_per_project").is_none());
        // Enum values are not keys, so kebab-case is preserved.
        assert_eq!(out["config"]["dependency"]["strategy"], "chain");
        assert!(out["expectedColdExecuted"].is_object());
        assert_eq!(
            out["projects"][0]["tasks"][0]["outputGlobs"][0],
            "dist/t0.*"
        );
    }

    #[test]
    fn build_model_ignores_ts_only_fields() {
        let out = build_model_value(json!({
            "projects": 2,
            "tasksPerProject": 1,
            "task": { "logLines": 5, "workIterations": 10, "outputFiles": 1 },
            "tools": ["omni", "turbo"],
            "versions": { "turbo": "2.0.0" }
        }))
        .unwrap();

        assert_eq!(out["projects"].as_array().unwrap().len(), 2);
        assert_eq!(out["config"]["task"]["outputFiles"], 1);
    }

    #[test]
    fn render_omni_consumes_camel_model() {
        let model = build_model_value(json!({
            "projects": 2,
            "tasksPerProject": 1,
            "dependency": { "strategy": "chain" }
        }))
        .unwrap();

        let files = render_omni_value(
            model,
            json!({
                "taskCommandTemplate": "node ./task.mjs {task_id}",
                "projectCacheKeyFiles": ["package.json", "task.mjs"]
            }),
        )
        .unwrap();

        let files = files.as_array().unwrap();
        assert_eq!(files[0][0], "workspace.omni.yaml");
        let (_, project_body) = files
            .iter()
            .filter_map(|pair| {
                let path = pair[0].as_str()?;
                let body = pair[1].as_str()?;
                Some((path, body))
            })
            .find(|(path, _)| path.ends_with("project.omni.yaml"))
            .unwrap();
        assert!(project_body.contains("node ./task.mjs t0"));
        assert!(project_body.contains("package.json"));
    }

    #[test]
    fn key_case_conversion_round_trips() {
        let snake = json!({ "edge_probability": 0.35, "nested": { "a_b": 1 } });
        let camel = keys_to_camel_case(snake.clone());
        assert_eq!(
            camel,
            json!({ "edgeProbability": 0.35, "nested": { "aB": 1 } })
        );
        assert_eq!(keys_to_snake_case(camel), snake);
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_tests {
    use super::*;
    use serde_json::json;
    use wasm_bindgen_test::*;

    #[wasm_bindgen_test]
    fn build_model_js_round_trips() {
        let config = serde_wasm_bindgen::to_value(&json!({
            "projects": 2,
            "tasksPerProject": 1,
            "dependency": { "strategy": "chain" }
        }))
        .unwrap();
        let model = build_model_js(config).expect("buildModel");
        let value: Value = serde_wasm_bindgen::from_value(model).unwrap();
        assert_eq!(value["modelVersion"], 1);
        assert_eq!(value["projects"].as_array().unwrap().len(), 2);
        assert_eq!(value["projects"][1]["dir"], "packages/p-1");
    }

    #[wasm_bindgen_test]
    fn render_omni_js_round_trips() {
        let config = serde_wasm_bindgen::to_value(&json!({
            "projects": 1,
            "tasksPerProject": 1
        }))
        .unwrap();
        let model = build_model_js(config).expect("buildModel");
        let options = serde_wasm_bindgen::to_value(&json!({
            "taskCommandTemplate": "node ./task.mjs {task_id}",
            "projectCacheKeyFiles": ["package.json"]
        }))
        .unwrap();
        let files = render_omni_js(model, options).expect("renderOmni");
        let files: Value = serde_wasm_bindgen::from_value(files).unwrap();
        assert_eq!(files[0][0], "workspace.omni.yaml");
    }
}
