use maps::UnorderedMap;
use strum::{EnumIs, EnumTryAs};

/// A configuration value expressed either uniformly for every key or per key.
///
/// Deserializes from either a single value (applied to *all* keys) or a map of
/// per-key values, mirroring [`SingleOrMany`](crate::SingleOrMany) for the
/// "blanket-or-keyed" shape:
///
/// ```yaml
/// # blanket form — one value governs every key
/// feature: true
///
/// # keyed form — a value per key
/// feature:
///   a: true
///   b: false
/// ```
#[derive(Debug, Clone, PartialEq, Eq, EnumIs, EnumTryAs)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", serde(untagged))]
#[cfg_attr(feature = "schemars", schemars(untagged))]
pub enum AllOrKeyed<T> {
    /// A single value applied uniformly to every key.
    All(T),
    /// Per-key values. A key absent from the map has no value.
    Keyed(UnorderedMap<String, T>),
}

impl<T> AllOrKeyed<T> {
    /// The value governing `key`: the blanket value in the [`All`](Self::All)
    /// form, or the entry for `key` in the [`Keyed`](Self::Keyed) form (an
    /// absent key yields `None`).
    pub fn get(&self, key: &str) -> Option<&T> {
        match self {
            AllOrKeyed::All(value) => Some(value),
            AllOrKeyed::Keyed(map) => map.get(key),
        }
    }

    /// The blanket value, if this is the [`All`](Self::All) form.
    pub fn all(&self) -> Option<&T> {
        match self {
            AllOrKeyed::All(value) => Some(value),
            AllOrKeyed::Keyed(_) => None,
        }
    }
}

impl<T: Clone> AllOrKeyed<T> {
    /// The value governing `key`, falling back to `default` when the keyed form
    /// has no entry for it.
    pub fn get_or(&self, key: &str, default: T) -> T {
        self.get(key).cloned().unwrap_or(default)
    }
}

impl<T: Default> Default for AllOrKeyed<T> {
    fn default() -> Self {
        AllOrKeyed::All(T::default())
    }
}

impl<T> From<T> for AllOrKeyed<T> {
    fn from(value: T) -> Self {
        AllOrKeyed::All(value)
    }
}

impl<T> From<UnorderedMap<String, T>> for AllOrKeyed<T> {
    fn from(value: UnorderedMap<String, T>) -> Self {
        AllOrKeyed::Keyed(value)
    }
}

#[cfg(feature = "merge")]
impl<T> merge::Merge for AllOrKeyed<T> {
    fn merge(&mut self, other: Self) {
        *self = other;
    }
}

#[cfg(all(test, feature = "serde"))]
mod tests {
    use super::*;

    #[test]
    fn all_form_deserializes_from_a_scalar() {
        let value: AllOrKeyed<bool> = serde_json::from_str("true").unwrap();
        assert_eq!(value, AllOrKeyed::All(true));
        assert_eq!(value.get("anything"), Some(&true));
    }

    #[test]
    fn keyed_form_deserializes_from_a_map() {
        let value: AllOrKeyed<bool> =
            serde_json::from_str(r#"{"a": true, "b": false}"#).unwrap();
        assert_eq!(value.get("a"), Some(&true));
        assert_eq!(value.get("b"), Some(&false));
        assert_eq!(value.get("c"), None);
        assert_eq!(value.get_or("c", false), false);
    }

    #[test]
    fn default_is_the_all_form_of_the_inner_default() {
        assert_eq!(AllOrKeyed::<bool>::default(), AllOrKeyed::All(false));
    }
}
