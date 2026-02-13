use std::{collections::HashMap, path::Path, sync::LazyLock};

use heck::*;
use tera::{Result, Tera, Value, to_value, try_get_value};

pub static FULL: LazyLock<Tera> = LazyLock::new(|| create_full_preset());

fn create_full_preset() -> tera::Tera {
    let mut tera = tera::Tera::default();

    register_all_filters(&mut tera);

    tera
}

// ripped from https://github.com/Keats/kickstart/blob/master/src/filters.rs
fn register_all_filters(tera: &mut Tera) {
    tera.register_filter("upper_camel_case", upper_camel_case);
    tera.register_filter("camel_case", camel_case);
    tera.register_filter("snake_case", snake_case);
    tera.register_filter("kebab_case", kebab_case);
    tera.register_filter("shouty_snake_case", shouty_snake_case);
    tera.register_filter("title_case", title_case);
    tera.register_filter("shouty_kebab_case", shouty_kebab_case);
    tera.register_filter("base_name", base_name);
    tera.register_filter("relative_path", relative_path);
}

pub fn upper_camel_case(
    value: &Value,
    _: &HashMap<String, Value>,
) -> Result<Value> {
    let s = try_get_value!("upper_camel_case", "value", String, value);
    Ok(to_value(s.to_upper_camel_case()).unwrap())
}

pub fn camel_case(value: &Value, _: &HashMap<String, Value>) -> Result<Value> {
    let s = try_get_value!("camel_case", "value", String, value);
    Ok(to_value(s.to_lower_camel_case()).unwrap())
}

pub fn snake_case(value: &Value, _: &HashMap<String, Value>) -> Result<Value> {
    let s = try_get_value!("snake_case", "value", String, value);
    Ok(to_value(s.to_snake_case()).unwrap())
}

pub fn kebab_case(value: &Value, _: &HashMap<String, Value>) -> Result<Value> {
    let s = try_get_value!("kebab_case", "value", String, value);
    Ok(to_value(s.to_kebab_case()).unwrap())
}

pub fn shouty_snake_case(
    value: &Value,
    _: &HashMap<String, Value>,
) -> Result<Value> {
    let s = try_get_value!("shouty_snake_case", "value", String, value);
    Ok(to_value(s.to_shouty_snake_case()).unwrap())
}

pub fn title_case(value: &Value, _: &HashMap<String, Value>) -> Result<Value> {
    let s = try_get_value!("title_case", "value", String, value);
    Ok(to_value(s.to_title_case()).unwrap())
}

pub fn shouty_kebab_case(
    value: &Value,
    _: &HashMap<String, Value>,
) -> Result<Value> {
    let s = try_get_value!("shouty_kebab_case", "value", String, value);
    Ok(to_value(s.to_shouty_kebab_case()).unwrap())
}

pub fn base_name(value: &Value, _: &HashMap<String, Value>) -> Result<Value> {
    let s = try_get_value!("base_name", "value", String, value);
    let path = Path::new(&s);
    Ok(to_value(path.file_name().map(|s| s.to_string_lossy())).unwrap())
}

pub fn relative_path(
    value: &Value,
    args: &HashMap<String, Value>,
) -> Result<Value> {
    let s = try_get_value!("relative_path", "value", String, value);
    let path = Path::new(&s);
    let root = Path::new(args["root"].as_str().ok_or_else(|| {
        tera::Error::msg("missing root argument or invalid type, must be present and be a string")
    })?);
    let relative_path = pathdiff::diff_paths(path, root).ok_or_else(|| {
        tera::Error::msg("unable to find relative path between root and path")
    })?;

    Ok(to_value(relative_path).unwrap())
}
