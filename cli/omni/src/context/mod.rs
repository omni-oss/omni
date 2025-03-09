use std::collections::HashMap;

use crate::commands::CliArgs;

pub struct Context {
    env: HashMap<String, String>,
}

impl Context {
    pub fn stop_at_root(&self) -> bool {
        self.env.contains_key("OMNI_STOP_AT_ROOT")
    }

    pub fn get_env(&self, key: &str) -> Option<&str> {
        self.env.get(key).map(|s| s.as_str())
    }

    pub fn set_env(
        &mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) {
        self.env.insert(key.into(), value.into());
    }

    pub fn remove_env(&mut self, key: &str) {
        self.env.remove(key);
    }

    pub fn get_all_env(&self) -> &HashMap<String, String> {
        &self.env
    }
}

pub fn build(_cli: &CliArgs) -> eyre::Result<Context> {
    let mut env = HashMap::new();
    env.insert("TEST".to_owned(), "VALUE".to_owned());
    Ok(Context { env })
}
