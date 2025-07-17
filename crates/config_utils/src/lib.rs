use std::collections::HashMap;

#[cfg(feature = "merge")]
use merge::Merge;

#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(untagged))]
pub enum ListConfig<T> {
    Value(Vec<T>),
    Merge { merge: Vec<T> },
    Replace { replace: Vec<T> },
}

impl<T> ListConfig<T> {
    pub fn merge_from_vec(&mut self, mut other: Vec<T>) {
        match self {
            ListConfig::Value(_) => {
                // do nothing
            }
            ListConfig::Merge { merge } => {
                std::mem::swap(merge, &mut other);
                merge.extend(other);
            }
            ListConfig::Replace { .. } => {
                // do nothing
            }
        }
    }

    pub fn into_vec(self) -> Vec<T> {
        match self {
            ListConfig::Value(items) => items,
            ListConfig::Merge { merge } => merge,
            ListConfig::Replace { replace } => replace,
        }
    }
}

#[cfg(feature = "merge")]
impl<T> Merge for ListConfig<T> {
    fn merge(&mut self, other: Self) {
        let vec = other.into_vec();

        self.merge_from_vec(vec);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(untagged))]
pub enum DictConfig<T> {
    Value(HashMap<String, T>),
    Merge { merge: HashMap<String, T> },
    Replace { replace: HashMap<String, T> },
}

impl<T> DictConfig<T> {
    fn merge_maps(map: &mut HashMap<String, T>, mut base: HashMap<String, T>) {
        std::mem::swap(map, &mut base);
        map.extend(base);
    }

    pub fn merge_from_map(&mut self, base: HashMap<String, T>) {
        match self {
            DictConfig::Value(merge) => {
                Self::merge_maps(merge, base);
            }
            DictConfig::Merge { merge } => {
                Self::merge_maps(merge, base);
            }
            DictConfig::Replace { .. } => {
                // do nothing
            }
        }
    }

    fn merge_maps_deep(map: &mut HashMap<String, T>, base: HashMap<String, T>)
    where
        T: Merge,
    {
        for (key, value) in base {
            if let Some(v) = map.get_mut(&key) {
                v.merge(value);
            } else {
                map.insert(key, value);
            }
        }
    }

    pub fn merge_deep_from_map(&mut self, base: HashMap<String, T>)
    where
        T: Merge,
    {
        match self {
            DictConfig::Value(merge) => {
                Self::merge_maps_deep(merge, base);
            }
            DictConfig::Merge { merge } => {
                Self::merge_maps_deep(merge, base);
            }
            DictConfig::Replace { .. } => {
                // do nothing
            }
        }
    }

    pub fn into_dict(self) -> HashMap<String, T> {
        match self {
            DictConfig::Value(items) => items,
            DictConfig::Merge { merge } => merge,
            DictConfig::Replace { replace } => replace,
        }
    }
}

#[cfg(feature = "merge")]
impl<T> Merge for DictConfig<T> {
    fn merge(&mut self, other: Self) {
        let dict = other.into_dict();

        self.merge_from_map(dict);
    }
}
