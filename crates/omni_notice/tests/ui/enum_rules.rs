use omni_notice::Notice;

#[derive(Notice)]
enum MissingCode {
    /// Missing code.
    #[notice(level = warn, message = "missing")]
    Missing,
}

#[derive(Notice)]
enum MissingLevel {
    /// Missing level.
    #[notice(code = "enum_test.missing_level", message = "missing")]
    Missing,
}

#[derive(Notice)]
enum MissingMessage {
    /// Missing message.
    #[notice(code = "enum_test.missing_message", level = info)]
    Missing,
}

/// Enum docs do not replace variant docs.
#[derive(Notice)]
enum MissingDocs {
    #[notice(code = "enum_test.missing_docs", level = info, message = "missing")]
    Missing,
}

#[derive(Notice)]
enum BadCode {
    /// Invalid code.
    #[notice(code = "INVALID", level = warn, message = "invalid")]
    Invalid,
}

#[derive(Notice)]
enum BadLevel {
    /// Invalid level.
    #[notice(code = "enum_test.bad_level", level = debug, message = "invalid")]
    Invalid,
}

#[derive(Notice)]
enum Duplicate {
    /// First.
    #[notice(code = "enum_test.same", level = info, message = "first")]
    First,
    /// Second.
    #[notice(code = "enum_test.same", level = warn, message = "second")]
    Second,
}

#[derive(Notice)]
#[notice(level = warn)]
enum EnumDefaults {}

#[derive(Notice)]
enum Tuple {
    /// Tuple fields are unsupported.
    #[notice(code = "enum_test.tuple", level = info, message = "tuple")]
    Tuple(String),
}

#[derive(Notice)]
enum FieldAttribute {
    /// Invalid field attribute.
    #[notice(code = "enum_test.field_attr", level = info, message = "field")]
    Named {
        #[notice(stable)]
        value: String,
    },
}

#[derive(Notice)]
enum EssentialFields {
    /// Skipped fields are still fields.
    #[notice(code = "enum_test.essential_fields", level = warn, essential,
             message = "static")]
    Named {
        #[notice(skip)]
        value: String,
    },
}

#[derive(Notice)]
enum EssentialPlaceholder {
    /// Placeholders are not static.
    #[notice(code = "enum_test.essential_placeholder", level = warn, essential,
             message = "{value}")]
    Unit,
}

#[derive(Notice)]
enum Generic<T> {
    Value(T),
}

#[derive(Notice)]
union Union {
    value: u32,
}

fn main() {}
