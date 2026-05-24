use std::path::Path;

pub use tera::{Context, Error, ErrorKind, Result, Template, Tera, Value};

use crate::preset::FULL;

pub mod context;
pub mod preset;

pub fn new(dir: &str) -> Result<Tera> {
    let mut tera = tera::Tera::new(dir)?;

    tera.extend(&FULL)?;

    Ok(tera)
}

pub fn new_with_files<F: AsRef<Path>, N: AsRef<str>>(
    files: &[(F, Option<N>)],
) -> Result<Tera> {
    let mut tera = Tera::default();

    tera.extend(&FULL)?;

    for (f, n) in files {
        tera.add_template_file(
            f.as_ref(),
            if let Some(n) = n {
                Some(n.as_ref())
            } else {
                None
            },
        )?;
    }

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
