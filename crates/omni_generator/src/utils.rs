use maps::UnorderedMap;
use value_bag::OwnedValueBag;

pub fn get_tera_context(
    context_values: &UnorderedMap<String, OwnedValueBag>,
) -> tera::Context {
    let mut context = tera::Context::new();

    for (key, value) in context_values.iter() {
        context.insert(key, value);
    }

    context
}
