//! Structured task identity (RFC 0024 F03.4 slice 0).
//!
//! A canonical task is its raw `(project, task)` pair. Equality and hashing
//! always compare both components by content. A `project#task` label is only a
//! display spelling ([`TaskLabel`]); it is never a map key.

use std::{collections::HashMap, fmt, sync::Arc};

use crate::task_reference::escape_label_into;

/// The escaped, round-trippable `project#task` spelling of a pair. Distinct
/// pairs render distinct labels, and `TaskReference::parse` of the label gives
/// back the same project and task.
///
/// The components must be non-empty and free of NUL; configuration and
/// discovery reject anything else before a task exists, so the formatter is
/// infallible.
pub fn task_label(project: &str, task: &str) -> String {
    let mut out = String::with_capacity(project.len() + task.len() + 1);
    escape_label_into(&mut out, project, task);
    out
}

/// A rendered task label for presentation. It deliberately has no `Eq`,
/// `Hash` or `Ord`, so it cannot become a map key by accident.
#[derive(Debug, Clone)]
pub struct TaskLabel(String);

impl TaskLabel {
    pub fn new(project: &str, task: &str) -> Self {
        Self(task_label(project, task))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for TaskLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn is_valid_name(name: &str) -> bool {
    !name.is_empty() && !name.contains('\0')
}

/// An owned `(project, task)` identity with cheaply clonable shared storage.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskKey {
    project: Arc<str>,
    task: Arc<str>,
}

impl TaskKey {
    pub fn new(
        project: impl Into<Arc<str>>,
        task: impl Into<Arc<str>>,
    ) -> Self {
        let project = project.into();
        let task = task.into();
        // Release builds rely on ingress validation (workspace loading rejects
        // empty and NUL names, `workspace.invalid_*_name`); construction only
        // asserts in debug builds so a bad stored name cannot abort a run.
        debug_assert!(
            is_valid_name(&project) && is_valid_name(&task),
            "empty or NUL project/task names are rejected at ingress"
        );
        Self { project, task }
    }

    pub fn project(&self) -> &str {
        &self.project
    }

    pub fn task(&self) -> &str {
        &self.task
    }

    pub fn as_ref(&self) -> TaskKeyRef<'_> {
        TaskKeyRef {
            project: &self.project,
            task: &self.task,
        }
    }

    pub fn label(&self) -> TaskLabel {
        TaskLabel::new(&self.project, &self.task)
    }
}

impl fmt::Display for TaskKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&task_label(&self.project, &self.task))
    }
}

/// A borrowed `(project, task)` identity for probes and scratch sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskKeyRef<'a> {
    pub project: &'a str,
    pub task: &'a str,
}

impl<'a> TaskKeyRef<'a> {
    pub fn new(project: &'a str, task: &'a str) -> Self {
        Self { project, task }
    }

    pub fn to_owned_key(self) -> TaskKey {
        TaskKey::new(self.project, self.task)
    }

    pub fn label(&self) -> TaskLabel {
        TaskLabel::new(self.project, self.task)
    }
}

impl<'a> From<&'a TaskKey> for TaskKeyRef<'a> {
    fn from(key: &'a TaskKey) -> Self {
        key.as_ref()
    }
}

impl fmt::Display for TaskKeyRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&task_label(self.project, self.task))
    }
}

/// Nested project/task map. Lookups borrow `&str` components and never build
/// an owned key or a label. It serializes as `{project: {task: value}}`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct TaskMap<V> {
    inner: HashMap<Arc<str>, HashMap<Arc<str>, V>>,
}

impl<V> Default for TaskMap<V> {
    fn default() -> Self {
        Self {
            inner: HashMap::new(),
        }
    }
}

impl<V> TaskMap<V> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, project: &str, task: &str, value: V) -> Option<V> {
        let bucket = match self.inner.get_mut(project) {
            Some(bucket) => bucket,
            None => self.inner.entry(Arc::from(project)).or_default(),
        };
        match bucket.get_mut(task) {
            Some(slot) => Some(std::mem::replace(slot, value)),
            None => {
                bucket.insert(Arc::from(task), value);
                None
            }
        }
    }

    /// Insert under `key`, sharing its storage instead of allocating new
    /// strings for a first-seen project or task.
    pub fn insert_key(&mut self, key: &TaskKey, value: V) -> Option<V> {
        let bucket = match self.inner.get_mut(key.project()) {
            Some(bucket) => bucket,
            None => self.inner.entry(Arc::clone(&key.project)).or_default(),
        };
        match bucket.get_mut(key.task()) {
            Some(slot) => Some(std::mem::replace(slot, value)),
            None => {
                bucket.insert(Arc::clone(&key.task), value);
                None
            }
        }
    }

    pub fn get(&self, project: &str, task: &str) -> Option<&V> {
        self.inner.get(project)?.get(task)
    }

    pub fn get_ref(&self, key: TaskKeyRef<'_>) -> Option<&V> {
        self.get(key.project, key.task)
    }

    pub fn get_mut(&mut self, project: &str, task: &str) -> Option<&mut V> {
        self.inner.get_mut(project)?.get_mut(task)
    }

    pub fn contains(&self, project: &str, task: &str) -> bool {
        self.get(project, task).is_some()
    }

    pub fn remove(&mut self, project: &str, task: &str) -> Option<V> {
        let bucket = self.inner.get_mut(project)?;
        let removed = bucket.remove(task);
        if bucket.is_empty() {
            self.inner.remove(project);
        }
        removed
    }

    pub fn entry_or_insert_with(
        &mut self,
        project: &str,
        task: &str,
        make: impl FnOnce() -> V,
    ) -> &mut V {
        if !self.contains(project, task) {
            self.insert(project, task, make());
        }
        self.get_mut(project, task).expect("just inserted")
    }

    /// All values of one project.
    pub fn project_tasks(
        &self,
        project: &str,
    ) -> impl Iterator<Item = (&str, &V)> {
        self.inner
            .get(project)
            .into_iter()
            .flat_map(|b| b.iter().map(|(t, v)| (&**t, v)))
    }

    pub fn len(&self) -> usize {
        self.inner.values().map(HashMap::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.values().all(HashMap::is_empty)
    }

    pub fn iter(&self) -> impl Iterator<Item = (TaskKeyRef<'_>, &V)> {
        self.inner.iter().flat_map(|(p, b)| {
            b.iter().map(move |(t, v)| (TaskKeyRef::new(p, t), v))
        })
    }

    /// Move every entry of `other` in, replacing equal pairs.
    pub fn extend(&mut self, other: TaskMap<V>) {
        for (project, bucket) in other.inner {
            self.inner.entry(project).or_default().extend(bucket);
        }
    }

    pub fn into_values(self) -> impl Iterator<Item = V> {
        self.inner.into_values().flat_map(HashMap::into_values)
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.inner.values().flat_map(HashMap::values)
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.inner.values_mut().flat_map(HashMap::values_mut)
    }
}

impl<V> FromIterator<(TaskKey, V)> for TaskMap<V> {
    fn from_iter<I: IntoIterator<Item = (TaskKey, V)>>(iter: I) -> Self {
        let mut map = Self::new();
        for (k, v) in iter {
            map.insert_key(&k, v);
        }
        map
    }
}

#[cfg(test)]
mod tests {
    use std::hash::{BuildHasher, Hasher};

    use super::*;
    use crate::TaskReference;

    #[derive(Clone, Default)]
    struct Colliding;
    struct ConstHasher;
    impl Hasher for ConstHasher {
        fn finish(&self) -> u64 {
            7
        }
        fn write(&mut self, _: &[u8]) {}
    }
    impl BuildHasher for Colliding {
        type Hasher = ConstHasher;
        fn build_hasher(&self) -> ConstHasher {
            ConstHasher
        }
    }

    #[test]
    fn equality_compares_components_not_hashes() {
        let mut map: HashMap<TaskKey, u8, Colliding> =
            HashMap::with_hasher(Colliding);
        map.insert(TaskKey::new("a#b", "c"), 1);
        map.insert(TaskKey::new("a", "b#c"), 2);
        map.insert(TaskKey::new("a", "b"), 3);
        assert_eq!(map.len(), 3);
        assert_eq!(map[&TaskKey::new("a#b", "c")], 1);
        assert_eq!(map[&TaskKey::new("a", "b#c")], 2);
    }

    #[test]
    fn formerly_colliding_pairs_are_distinct_keys_and_labels() {
        let first = TaskKey::new("a#b", "c");
        let second = TaskKey::new("a", "b#c");
        assert_ne!(first, second);
        assert_eq!(first.to_string(), r"a\#b#c");
        assert_eq!(second.to_string(), r"a#b\#c");
    }

    #[test]
    fn plain_names_render_as_before() {
        assert_eq!(task_label("app", "build"), "app#build");
        assert_eq!(task_label("app", "lint:fix"), "app#lint:fix");
    }

    #[test]
    fn label_round_trips_through_the_reference_grammar() {
        for (p, t) in [
            ("a#b", "c"),
            ("a", "b#c"),
            ("a\\b", "c\\"),
            ("^up", "x"),
            ("p", "a::b"),
            ("p:", ":t"),
            ("é", "🧪"),
        ] {
            let parsed = TaskReference::parse(&task_label(p, t)).unwrap();
            assert_eq!(parsed.project(), Some(p), "{p:?} {t:?}");
            assert_eq!(parsed.task(), t, "{p:?} {t:?}");
        }
    }

    #[test]
    fn task_map_probes_with_borrowed_names() {
        let mut map = TaskMap::new();
        assert_eq!(map.insert("a#b", "c", 1), None);
        assert_eq!(map.insert("a", "b#c", 2), None);
        assert_eq!(map.insert("a", "b#c", 3), Some(2));
        assert_eq!(map.get("a#b", "c"), Some(&1));
        assert_eq!(map.get("a", "b#c"), Some(&3));
        assert_eq!(map.get("a", "c"), None);
        assert_eq!(map.len(), 2);
        assert_eq!(map.remove("a", "b#c"), Some(3));
        assert!(!map.contains("a", "b#c"));
    }

    #[cfg(feature = "serde")]
    #[test]
    fn a_deserialized_map_of_empty_buckets_is_empty() {
        let map: TaskMap<u8> = serde_json::from_str(r#"{"p":{}}"#).unwrap();
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
    }
}
