use tera::{Result, Tera};

use crate::preset::FULL;

pub mod preset;

pub fn new(dir: &str) -> Result<Tera> {
    let mut tera = tera::Tera::new(dir)?;

    tera.extend(&FULL)?;

    Ok(tera)
}

pub fn one_off<I: AsRef<str>, N: AsRef<str>>(
    input: I,
    name: N,
    context: &tera::Context,
) -> Result<String> {
    let mut tera = Tera::default();

    tera.extend(&FULL)?;

    tera.add_raw_template(name.as_ref(), input.as_ref())?;

    let rendered = tera.render(name.as_ref(), context)?;

    Ok(rendered)
}
