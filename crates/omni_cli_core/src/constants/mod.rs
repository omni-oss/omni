use lazy_regex::{Lazy, regex};
use regex::Regex;

pub const SUPPORTED_EXTENSIONS: [&str; 4] = ["yml", "yaml", "json", "toml"];
pub const WORKSPACE_OMNI: &str = "workspace.omni.{ext}";
pub const PROJECT_OMNI: &str = "project.omni.{ext}";
pub const OMNI_IGNORE: &str = ".omniignore";
pub const WORKSPACE_DIR_VAR: &str = "WORKSPACE_DIR";
pub const PROJECT_DIR_VAR: &str = "PROJECT_DIR";

// Regex Patterns
pub static PROJECT_NAME_REGEX: &Lazy<Regex> = regex!(r#"""[/\.\@\:\w\-]+"""#);
pub static WORKSPACE_NAME_REGEX: &Lazy<Regex> = regex!(r#"""[/\.\@\:\w\-]+"""#);
pub static TASK_NAME_REGEX: &Lazy<Regex> = regex!(r#"""[/\.\@\:\w\-]+"""#);
pub static TASK_DEPENDENCY_REGEX: &Lazy<Regex> = regex!(
    r#"((?<explicit_project>[/\.\@\:\w\-]+)#(?<explicit_task>[/\.\@\:\w\-]+))|(\^(?<upstream_task>[/\.\@\:\w-]+))|(?<own_task>[/\.\@\:\w\-]+)"#
);
