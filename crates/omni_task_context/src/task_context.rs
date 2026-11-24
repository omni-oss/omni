use std::{borrow::Cow, sync::Arc};

use omni_config_types::TeraExprBoolean;
use omni_core::TaskExecutionNode;
use omni_hasher::impls::DefaultHash;
use omni_types::OmniPath;

use crate::{CacheInfo, aliases::EnvVars};

#[derive(Debug, Clone)]
pub struct TaskContext<'a> {
    pub node: &'a TaskExecutionNode,
    pub dependency_hashes: Vec<DefaultHash>,
    pub env_vars: Arc<EnvVars>,
    pub cache_info: Option<Cow<'a, CacheInfo>>,
    pub output_logs: Option<omni_task_output_logs::OutputLogsConfiguration>,
    pub template_context: omni_tera::Context,
}

impl<'a> TaskContext<'a> {
    /// Render cache paths using the same context as task execution.
    pub fn expand_cache_paths(
        &self,
    ) -> omni_tera::Result<Cow<'_, TaskContext<'a>>> {
        let Some(info) = &self.cache_info else {
            return Ok(Cow::Borrowed(self));
        };
        let has_template = |paths: &[OmniPath]| {
            paths.iter().any(|path| {
                path.unresolved_path()
                    .as_os_str()
                    .as_encoded_bytes()
                    .contains(&b'{')
            })
        };
        if !has_template(&info.cache_output_files.include)
            && !has_template(&info.cache_output_files.exclude)
            && !has_template(&info.key_input_files.include)
            && !has_template(&info.key_input_files.exclude)
        {
            return Ok(Cow::Borrowed(self));
        }
        let expand = |paths: &[OmniPath]| {
            paths
                .iter()
                .map(|path| {
                    let text = path.unresolved_path().to_str().unwrap_or("");
                    let expanded = omni_tera::one_off(
                        text,
                        "path",
                        &self.template_context,
                    )?;
                    Ok(match path.root() {
                        Some(root) => OmniPath::new_rooted(expanded, root),
                        None => OmniPath::new(expanded),
                    })
                })
                .collect::<omni_tera::Result<Vec<_>>>()
        };
        let mut info = info.as_ref().clone();
        info.cache_output_files.include =
            expand(&info.cache_output_files.include)?;
        info.cache_output_files.exclude =
            expand(&info.cache_output_files.exclude)?;
        info.key_input_files.include = expand(&info.key_input_files.include)?;
        info.key_input_files.exclude = expand(&info.key_input_files.exclude)?;
        let mut context = self.clone();
        context.cache_info = Some(Cow::Owned(info));
        Ok(Cow::Owned(context))
    }

    /// Evaluate the task's enabled condition in its own template context.
    pub fn is_enabled(&self) -> omni_tera::Result<bool> {
        Ok(match self.node.enabled() {
            TeraExprBoolean::Boolean(enabled) => *enabled,
            TeraExprBoolean::Expr(expr) => {
                if let Some(enabled) = static_enabled(expr.as_str()) {
                    return Ok(enabled);
                }
                let result = omni_tera::one_off(
                    expr.as_str(),
                    "expr",
                    &self.template_context,
                )?;
                result.trim().eq_ignore_ascii_case("true")
            }
        })
    }
}

/// The value of an `enabled` expression that needs no rendering, if it has one.
/// Shared by task execution and the static cache inspection so they agree.
pub fn static_enabled(expr: &str) -> Option<bool> {
    match expr.trim().to_lowercase().as_str() {
        "true" | "yes" | "y" | "1" => Some(true),
        "false" | "no" | "n" | "0" => Some(false),
        _ => None,
    }
}

/// How a task's `enabled` setting reads from configuration alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaticEnabled {
    Enabled,
    Disabled,
    /// A template expression that is only known at execution time.
    Conditional,
}

pub fn classify_enabled(enabled: &TeraExprBoolean) -> StaticEnabled {
    match enabled {
        TeraExprBoolean::Boolean(true) => StaticEnabled::Enabled,
        TeraExprBoolean::Boolean(false) => StaticEnabled::Disabled,
        TeraExprBoolean::Expr(expr) => match static_enabled(expr.as_str()) {
            Some(true) => StaticEnabled::Enabled,
            Some(false) => StaticEnabled::Disabled,
            None => StaticEnabled::Conditional,
        },
    }
}

#[cfg(test)]
mod enabled_tests {
    use super::*;

    #[test]
    fn classifies_without_rendering() {
        let c =
            |s: &str| classify_enabled(&TeraExprBoolean::from(s.to_string()));
        assert_eq!(c(" FALSE "), StaticEnabled::Disabled);
        assert_eq!(c("No"), StaticEnabled::Disabled);
        assert_eq!(c("n"), StaticEnabled::Disabled);
        assert_eq!(c("0"), StaticEnabled::Disabled);
        assert_eq!(c("Yes"), StaticEnabled::Enabled);
        assert_eq!(c("{{ env.CI }}"), StaticEnabled::Conditional);
        assert_eq!(
            classify_enabled(&TeraExprBoolean::Boolean(false)),
            StaticEnabled::Disabled
        );
        assert_eq!(
            classify_enabled(&TeraExprBoolean::Boolean(true)),
            StaticEnabled::Enabled
        );
    }
}
