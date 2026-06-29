use std::path::Path;

pub use tera::{Context, Error, ErrorKind, Tera, TeraResult as Result, Value};

use crate::preset::FULL;

pub mod context;
pub mod preset;

pub fn new_with_files<F: AsRef<Path>, N: AsRef<str>>(
    files: &[(F, Option<N>)],
) -> Result<Tera> {
    let mut tera = FULL.clone();

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
    let mut tera = FULL.clone();

    tera.add_raw_template(name.as_ref(), input.as_ref())?;

    let rendered = tera.render(name.as_ref(), context)?;

    Ok(rendered)
}
