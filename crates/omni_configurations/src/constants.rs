use lazy_regex::{Lazy, Regex, regex};

// Regex Patterns
pub static WORKSPACE_NAME_REGEX: &Lazy<Regex> = regex!(r#"""[/\.\@\:\w\-]+"""#);
