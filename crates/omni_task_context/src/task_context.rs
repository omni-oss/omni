use std::{borrow::Cow, sync::Arc};

use omni_config_types::TeraExprBoolean;
use omni_core::TaskExecutionNode;
use omni_hasher::impls::DefaultHash;

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

impl TaskContext<'_> {
    /// Evaluate the task's enabled condition in its own template context.
    pub fn is_enabled(&self) -> omni_tera::Result<bool> {
        Ok(match self.node.enabled() {
            TeraExprBoolean::Boolean(enabled) => *enabled,
            TeraExprBoolean::Expr(expr) => {
                match expr.as_str().trim().to_lowercase().as_str() {
                    "true" | "yes" | "y" | "1" => return Ok(true),
                    "false" | "no" | "n" | "0" => return Ok(false),
                    _ => {}
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
