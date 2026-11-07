use omni_notice::Notice;

#[derive(Debug, Notice)]
enum Invalid {
    /// Notices cannot also be errors.
    #[notice(code = "enum_test.error", level = error, message = "invalid")]
    Error,
}

impl std::error::Error for Invalid {}

fn main() {}
