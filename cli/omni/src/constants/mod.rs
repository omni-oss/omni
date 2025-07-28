pub const SUPPORTED_EXTENSIONS: [&str; 4] = ["yml", "yaml", "json", "toml"];
pub const WORKSPACE_OMNI: &str = "workspace.omni.{ext}";
pub const PROJECT_OMNI: &str = "project.omni.{ext}";
pub const OMNI_IGNORE: &str = ".omniignore";
pub const WORKSPACE_DIR_VAR: &str = "WORKSPACE_DIR";
pub const PROJECT_DIR_VAR: &str = "PROJECT_DIR";

// Regex Patterns
pub const PROJECT_NAME_REGEX: &str = r#"""[/\.\@\:\w\-]+"""#;
pub const TASK_NAME_REGEX: &str = r#"""[/\.\@\:\w\-]+"""#;
pub const TASK_DEPENDENCY_REGEX: &str = r#"((?<explicit_project>[/\.\@\:\w\-]+)#(?<explicit_task>[/\.\@\:\w\-]+))|(\^(?<upstream_task>[/\.\@\:\w-]+))|(?<own_task>[/\.\@\:\w\-]+)"#;
