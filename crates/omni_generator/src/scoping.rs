use std::borrow::Cow;

use omni_generator_configurations::GeneratorConfiguration;

pub fn assign_scope_id<S: ToString>(
    scope_id: S,
    configs: Vec<Cow<GeneratorConfiguration>>,
) -> Vec<Cow<'static, GeneratorConfiguration>> {
    let scope_id = scope_id.to_string();
    configs
        .into_iter()
        .map(|c| {
            let mut c = c.into_owned();

            c.scope_id = Some(scope_id.clone());

            Cow::Owned(c)
        })
        .collect::<Vec<_>>()
}
