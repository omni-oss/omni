use std::collections::HashMap;

use derive_more::Constructor;

#[derive(Constructor)]
pub struct ParseConfig<'a> {
    pub expand: bool,
    pub extra_envs: Option<&'a HashMap<String, String>>,
}

impl<'a> Default for ParseConfig<'a> {
    fn default() -> Self {
        Self {
            expand: true,
            extra_envs: None,
        }
    }
}
