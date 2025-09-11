use derive_new::new;
use maps::Map;

use crate::CommandExpansionConfig;

#[derive(new)]
pub struct ParseConfig<'a> {
    pub expand: bool,
    pub command_expand: Option<&'a CommandExpansionConfig<'a>>,
    pub extra_envs: Option<&'a Map<String, String>>,
}

impl<'a> Default for ParseConfig<'a> {
    fn default() -> Self {
        Self {
            expand: true,
            command_expand: None,
            extra_envs: None,
        }
    }
}
