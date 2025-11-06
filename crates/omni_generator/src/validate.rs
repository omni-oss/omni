use omni_generator_configurations::GeneratorConfiguration;
use sets::unordered_set;

use crate::error::{Error, ErrorInner};

pub fn validate(generators: &[GeneratorConfiguration]) -> Result<(), Error> {
    let mut names = unordered_set!();

    for generator in generators {
        if names.contains(&generator.name) {
            return Err(ErrorInner::new_duplicate_generator_name(
                generator.name.clone(),
                generator.file.clone(),
            ))?;
        }

        names.insert(generator.name.clone());
    }

    Ok(())
}
