use std::collections::{HashMap, HashSet};

use omni_hasher::impls::DefaultHash;

/// A union of task definitions and current digests over every cache-sharing
/// workspace. A definition without a digest prevents stale classification.
#[derive(Debug, Default)]
pub struct PruneInventory {
    projects: HashMap<String, HashMap<String, TaskObservations>>,
}

#[derive(Debug, Default)]
struct TaskObservations {
    digests: HashSet<DefaultHash>,
    indeterminate: bool,
}

impl PruneInventory {
    pub(crate) fn observe(
        &mut self,
        project: &str,
        task: &str,
        digest: Option<DefaultHash>,
    ) {
        let observation = self
            .projects
            .entry(project.to_owned())
            .or_default()
            .entry(task.to_owned())
            .or_default();
        if let Some(digest) = digest {
            observation.digests.insert(digest);
        } else {
            observation.indeterminate = true;
        }
    }

    fn task(&self, project: &str, task: &str) -> Option<&TaskObservations> {
        self.projects.get(project)?.get(task)
    }

    #[cfg(test)]
    pub(crate) fn is_indeterminate(&self, project: &str, task: &str) -> bool {
        self.task(project, task).is_some_and(|e| e.indeterminate)
    }

    #[cfg(test)]
    pub(crate) fn current_digests(
        &self,
        project: &str,
        task: &str,
    ) -> Vec<DefaultHash> {
        self.task(project, task)
            .map(|e| e.digests.iter().copied().collect())
            .unwrap_or_default()
    }

    pub(crate) fn is_orphan(&self, project: &str, task: &str) -> bool {
        self.task(project, task).is_none()
    }

    pub(crate) fn is_stale(
        &self,
        project: &str,
        task: &str,
        digest: &DefaultHash,
    ) -> bool {
        self.task(project, task).is_some_and(|entry| {
            !entry.indeterminate && !entry.digests.contains(digest)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_current_digests_from_every_checkout_and_indeterminate_tasks() {
        let mut inventory = PruneInventory::default();
        inventory.observe("app", "build", Some([1; 32]));
        inventory.observe("app", "build", Some([2; 32]));
        inventory.observe("app", "disabled", None);
        inventory.observe("app", "disabled", Some([1; 32]));

        assert!(!inventory.is_stale("app", "build", &[1; 32]));
        assert!(!inventory.is_stale("app", "build", &[2; 32]));
        assert!(inventory.is_stale("app", "build", &[3; 32]));
        assert!(!inventory.is_stale("app", "disabled", &[3; 32]));
        assert!(!inventory.is_orphan("app", "disabled"));
        assert!(!inventory.is_stale("app", "removed", &[3; 32]));
        assert!(inventory.is_orphan("app", "removed"));
    }
}
