use omni_input_provider::{bon_builder_extend_multiple, builder::*};

use crate::{AllowedValueExtras, ArrayExtras, GenBase, Generator, ListWidget};

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

bon_builder_extend_multiple!(
    profile = Generator,
    bases = [
        {
            builder    = StringBuilder,
            set_method = base_extra,
        },
        {
            builder    = IntegerBuilder,
            set_method = base_extra,
        },
        {
            builder    = BooleanBuilder,
            set_method = base_extra,
        },
        {
            builder    = FloatBuilder,
            set_method = base_extra,
        },
        {
            builder    = StringArrayBuilder,
            set_method = base_extra,
        },
        {
            builder    = IntegerArrayBuilder,
            set_method = base_extra,
        },
        {
            builder    = FloatArrayBuilder,
            set_method = base_extra,
        },
        {
            builder    = ObjectBuilder,
            set_method = base_extra,
        },
    ],
    mixin = {
        ctor = gen_base,
    },
);

bon_builder_extend_multiple!(
    profile = Generator,
    bases = [
        {
            builder    = AllowedBuilder,
            set_method = base_extra,
        },
    ],
    mixin = {
        ctor = allowed_extras,
    },
);

bon_builder_extend_multiple!(
    profile = Generator,
    bases = [
        {
            builder    = StringArrayBuilder,
            set_method = array_extra,
        },
        {
            builder    = IntegerArrayBuilder,
            set_method = array_extra,
        },
        {
            builder    = FloatArrayBuilder,
            set_method = array_extra,
        },
    ],
    mixin = {
        ctor = array_extras,
    },
);
