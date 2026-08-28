use lazy_regex::{Lazy, regex};
use regex::Regex;

pub use omni_constants::{
    OMNI_IGNORE, PROJECT_OMNI, SUPPORTED_CONFIG_EXTS as SUPPORTED_EXTENSIONS,
    WORKSPACE_OMNI,
};

pub const WORKSPACE_DIR_VAR: &str = "WORKSPACE_DIR";
pub const PROJECT_DIR_VAR: &str = "PROJECT_DIR";

// Regex Patterns
pub static PROJECT_NAME_REGEX: &Lazy<Regex> = regex!(r#"""[/\.\@\:\w\-]+"""#);
pub static WORKSPACE_NAME_REGEX: &Lazy<Regex> = regex!(r#"""[/\.\@\:\w\-]+"""#);
pub static TASK_NAME_REGEX: &Lazy<Regex> = regex!(r#"""[/\.\@\:\w\-]+"""#);
pub static TASK_DEPENDENCY_REGEX: &Lazy<Regex> = regex!(
    r#"((?<explicit_project>[/\.\@\:\w\-]+)#(?<explicit_task>[/\.\@\:\w\-]+))|(\^(?<upstream_task>[/\.\@\:\w-]+))|(?<own_task>[/\.\@\:\w\-]+)"#
);
