use std::{path::Path, sync::LazyLock};

use cruet::Inflector;
use heck::ToShoutyKebabCase;
use tera::{Kwargs, State, Tera, TeraResult as Result};
use tera_contrib::{
    base64::{b64_decode, b64_encode},
    dates::{date, is_after, is_before, now},
    format::format,
    json::json_encode,
    rand::{get_random, shuffle},
    regex::{spaceless, striptags},
    slug::slug,
    urlencode::{urlencode, urlencode_strict},
};

pub static FULL: LazyLock<Tera> = LazyLock::new(|| create_full_preset());

fn create_full_preset() -> tera::Tera {
    let mut tera = tera::Tera::default();

    register_all(&mut tera);

    tera
}

pub(crate) fn register_all(tera: &mut Tera) {
    // contrib
    tera.register_filter("json_encode", json_encode);
    tera.register_filter("b64_encode", b64_encode);
    tera.register_filter("b64_decode", b64_decode);
    tera.register_function("now", now);
    tera.register_filter("date", date);
    tera.register_test("is_after", is_after);
    tera.register_test("is_before", is_before);
    tera.register_filter("format", format);
    tera.register_function("get_random", get_random);
    tera.register_filter("shuffle", shuffle);
    tera.register_filter("spaceless", spaceless);
    tera.register_filter("striptags", striptags);
    tera.register_filter("slug", slug);
    tera.register_filter("urlencode", urlencode);
    tera.register_filter("urlencode_strict", urlencode_strict);

    // custom
    tera.register_filter("pascal_case", pascal_case);
    tera.register_filter("camel_case", camel_case);
    tera.register_filter("snake_case", snake_case);
    tera.register_filter("kebab_case", kebab_case);
    tera.register_filter("screaming_snake_case", screaming_snake_case);
    tera.register_filter("title_case", title_case);
    tera.register_filter("screaming_kebab_case", screaming_kebab_case);
    tera.register_filter("sentence_case", sentence_case);
    tera.register_filter("table_case", table_case);
    tera.register_filter("class_case", class_case);
    tera.register_filter("train_case", train_case);
    tera.register_filter("base_name", base_name);
    tera.register_filter("relative_path", relative_path);
    tera.register_filter("plural", plural);
    tera.register_filter("singular", singular);
    tera.register_filter("ordinalize", ordinal);
    tera.register_filter("deordinalize", deordinal);
}

pub fn pascal_case(value: &str, _: Kwargs, _: &State) -> String {
    value.to_pascal_case()
}

pub fn camel_case(value: &str, _: Kwargs, _: &State) -> String {
    value.to_camel_case()
}

pub fn snake_case(value: &str, _: Kwargs, _: &State) -> String {
    value.to_snake_case()
}

pub fn kebab_case(value: &str, _: Kwargs, _: &State) -> String {
    value.to_kebab_case()
}

pub fn screaming_snake_case(value: &str, _: Kwargs, _: &State) -> String {
    value.to_screaming_snake_case()
}

pub fn title_case(value: &str, _: Kwargs, _: &State) -> String {
    value.to_title_case()
}

pub fn screaming_kebab_case(value: &str, _: Kwargs, _: &State) -> String {
    value.to_shouty_kebab_case()
}

pub fn sentence_case(value: &str, _: Kwargs, _: &State) -> String {
    value.to_sentence_case()
}

pub fn table_case(value: &str, _: Kwargs, _: &State) -> String {
    value.to_table_case()
}

pub fn class_case(value: &str, _: Kwargs, _: &State) -> String {
    value.to_class_case()
}

pub fn train_case(value: &str, _: Kwargs, _: &State) -> String {
    value.to_train_case()
}

pub fn base_name(value: &str, _: Kwargs, _: &State) -> String {
    let path = Path::new(&value);
    path.file_name()
        .map(|s| s.to_string_lossy().to_string())
        .expect("base_name: value must be a valid path")
}

pub fn relative_path(value: &str, args: Kwargs, _: &State) -> Result<String> {
    let path = omni_utils::path::clean(Path::new(&value));
    let root =
        omni_utils::path::clean(Path::new(&args.must_get::<&str>("root")?));

    log::debug!(
        "calculated relative path between root {root:?} and value {path:?}"
    );
    let relative_path =
        pathdiff::diff_paths(&path, &root).ok_or_else(|| {
            tera::Error::message(
                format!("unable to find relative path between root ({root:?}) and path ({path:?})")
            )
        })?;
    log::debug!(
        "calculated relative path between root {:?} and value {:?}, result: {:?}",
        root,
        path,
        relative_path,
    );

    Ok(relative_path.to_string_lossy().to_string())
}

pub fn plural(value: &str, _: Kwargs, _: &State) -> String {
    value.to_plural()
}

pub fn singular(value: &str, _: Kwargs, _: &State) -> String {
    value.to_singular()
}

pub fn ordinal(value: &str, _: Kwargs, _: &State) -> String {
    value.ordinalize()
}

pub fn deordinal(value: &str, _: Kwargs, _: &State) -> String {
    value.deordinalize()
}
