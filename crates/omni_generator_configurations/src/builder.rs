use crate::{AllowedValueExtras, ArrayExtras, GenBase, ListWidget};

#[::bon::builder(finish_fn = build)]
pub fn allowed_extras(
    #[builder(into)] name: Option<String>,
    #[builder(into)] separator: Option<bool>,
) -> AllowedValueExtras {
    AllowedValueExtras {
        name,
        separator: separator.unwrap_or(false),
    }
}

#[::bon::builder(finish_fn = build)]
pub fn array_extras(
    #[builder(into)] widget: Option<ListWidget>,
) -> ArrayExtras {
    ArrayExtras { widget }
}

#[::bon::builder(finish_fn = build)]
pub fn gen_base(
    #[builder(into)] message: String,
    #[builder(default, into)] remember: bool,
) -> GenBase {
    GenBase { message, remember }
}
