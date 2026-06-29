use std::sync::LazyLock;

use serde::Serialize;
use tera::Context;
pub use tera::context;

#[derive(Serialize)]
struct Platform {
    host: String,
    os: String,
    arch: String,
    vendor: String,
    binary_format: String,
}

impl Platform {
    fn infer() -> Self {
        Self {
            host: target_lexicon::HOST.to_string(),
            os: target_lexicon::HOST.operating_system.to_string(),
            arch: target_lexicon::HOST.architecture.to_string(),
            binary_format: target_lexicon::HOST.binary_format.to_string(),
            vendor: target_lexicon::HOST.vendor.to_string(),
        }
    }
}

fn standard() -> Context {
    let mut ctx = Context::new();

    ctx.insert("platform", &Platform::infer());

    ctx
}

pub static STANDARD: LazyLock<Context> = LazyLock::new(standard);
