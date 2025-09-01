use lazy_regex::{Lazy, Regex, regex};

// Regex Patterns
pub static WORKSPACE_NAME_REGEX: &Lazy<Regex> = regex!(r#"""[/\.\@\:\w\-]+"""#);
pub static TASK_DEPENDENCY_REGEX: &Lazy<Regex> = regex!(
    r#"((?<explicit_project>[/\.\@\:\w\-]+)#(?<explicit_task>[/\.\@\:\w\-]+))|(\^(?<upstream_task>[/\.\@\:\w-]+))|(?<own_task>[/\.\@\:\w\-]+)"#
);
