use derive_new::new;
use serde::{Deserialize, Serialize};

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Deserialize,
    Serialize,
    new,
)]
#[serde(transparent)]
pub struct ResponseStatusCode(u16);

#[macro_export]
macro_rules! predefined_status_codes {
    (
        $(
            $name:ident = $value:expr
        ),*$(,)?
    ) => {
        $(
            pub const $name: ResponseStatusCode = ResponseStatusCode($value);
        )*
    };
}

impl ResponseStatusCode {
    predefined_status_codes!(SUCCESS = 200, NOT_FOUND = 404);
}
