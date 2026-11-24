use std::{error::Error, sync::Arc};

use maps::UnorderedMap;
use omni_core::TaskExecutionNode;
use omni_hasher::impls::DefaultHash;
use omni_types::{TaskKey, TaskKeyRef};

use crate::{CacheInfo, TaskContext, aliases::EnvVars};

pub trait TaskHashProvider {
    fn get_task_hash(&self, task: TaskKeyRef<'_>) -> Option<DefaultHash>;

    /// The digests a dependent should be keyed on for this dependency. A node
    /// with no hash of its own (a commandless aggregate) may forward the
    /// digests of its own dependencies.
    fn get_dependency_hashes(&self, task: TaskKeyRef<'_>) -> Vec<DefaultHash> {
        self.get_task_hash(task).into_iter().collect()
    }
}

/// How a dependency contributes to its caller's cache key.
pub enum DependencyHash<'a> {
    Direct(Option<DefaultHash>),
    Forward(&'a [TaskKey]),
}

/// Disabled and commandless nodes forward their dependencies in plan order.
/// A diamond contributes each transitive dependency only once per call. The walk
/// is by `(project, task)` identity.
pub fn collect_dependency_hashes<'a>(
    task: TaskKeyRef<'a>,
    lookup: impl for<'k> Fn(TaskKeyRef<'k>) -> DependencyHash<'a>,
) -> Vec<DefaultHash> {
    let mut hashes = Vec::new();
    let mut seen: std::collections::HashSet<TaskKeyRef<'a>> =
        std::collections::HashSet::new();
    let mut stack = vec![task];
    while let Some(name) = stack.pop() {
        if !seen.insert(name) {
            continue;
        }
        match lookup(name) {
            DependencyHash::Direct(hash) => hashes.extend(hash),
            DependencyHash::Forward(deps) => {
                stack.extend(deps.iter().rev().map(TaskKey::as_ref));
            }
        }
    }
    hashes
}

pub trait Context {
    type Error: Error + Send + Sync + 'static;

    fn get_task_env_vars(
        &self,
        node: &TaskExecutionNode,
    ) -> Result<Option<Arc<EnvVars>>, Self::Error>;

    fn get_task_meta_config(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> Option<&omni_configurations::MetaConfiguration>;

    fn get_project_meta_config(
        &self,
        project_name: &str,
    ) -> Option<&omni_configurations::MetaConfiguration>;

    fn get_cache_info(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> Option<&CacheInfo>;

    fn get_output_logs(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> Option<&omni_task_output_logs::OutputLogsConfiguration>;

    fn get_task_override_args(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> Option<&UnorderedMap<String, serde_json::Value>>;
}

pub trait TaskContextProvider<'a>: 'a {
    type Error: Error + Send + Sync + 'static;

    fn get_task_context(
        &'a self,
        node: &'a TaskExecutionNode,
        ignore_dependencies: bool,
    ) -> Result<TaskContext<'a>, Self::Error>;
}

pub trait TaskContextProviderExt<'a>: TaskContextProvider<'a> {
    fn get_task_contexts(
        &'a self,
        nodes: &'a [TaskExecutionNode],
        ignore_dependencies: bool,
    ) -> Result<Vec<TaskContext<'a>>, Self::Error> {
        nodes
            .iter()
            .map(|n| self.get_task_context(n, ignore_dependencies))
            .collect()
    }
}

impl<'a, T: TaskContextProvider<'a>> TaskContextProviderExt<'a> for T {}

#[cfg(test)]
mod tests {
    use omni_types::TaskMap;

    use super::*;

    #[test]
    fn forwarding_walks_formerly_colliding_pairs_separately() {
        let ab = TaskKey::new("a#b", "c");
        let a = TaskKey::new("a", "b#c");
        let root = TaskKey::new("app", "x");
        let mut hashes = TaskMap::new();
        hashes.insert("a#b", "c", [1; 32]);
        hashes.insert("a", "b#c", [2; 32]);
        let mut forward = TaskMap::new();
        forward.insert("app", "x", vec![a.clone(), ab.clone(), a.clone()]);

        let collected = collect_dependency_hashes(root.as_ref(), |k| {
            if let Some(deps) = forward.get_ref(k) {
                DependencyHash::Forward(deps)
            } else {
                DependencyHash::Direct(hashes.get_ref(k).copied())
            }
        });

        assert_eq!(collected, vec![[2; 32], [1; 32]]);
    }
}
