use derive_more::Constructor;
use maps::Map;

#[derive(Constructor)]
pub struct ParseConfig<'a> {
    pub expand: bool,
    pub extra_envs: Option<&'a Map<String, String>>,
}

impl<'a> Default for ParseConfig<'a> {
    fn default() -> Self {
        Self {
            expand: true,
            extra_envs: None,
        }
    }
}
