use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(
    Serialize,
    Deserialize,
    Debug,
    Clone,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    JsonSchema,
)]
#[serde(untagged)]
pub enum ListConfig<T> {
    Value(Vec<T>),
    Append { append: Vec<T> },
    Prepend { prepend: Vec<T> },
    PrependAndAppend { append: Vec<T>, prepend: Vec<T> },
    Replace { replace: Vec<T> },
}

impl<T> ListConfig<T> {
    pub fn merge(self, mut other: Vec<T>) -> Vec<T> {
        match self {
            ListConfig::Value(items) => items,
            ListConfig::Append { append } => {
                other.extend(append);

                other
            }
            ListConfig::Prepend { mut prepend } => {
                prepend.extend(other);

                prepend
            }
            ListConfig::PrependAndAppend {
                append,
                mut prepend,
            } => {
                other.extend(append);

                prepend.extend(other);

                prepend
            }
            ListConfig::Replace { replace } => replace,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
#[serde(untagged)]
pub enum DictConfig<T> {
    Value(HashMap<String, T>),
    Merge { merge: HashMap<String, T> },
    Replace { replace: HashMap<String, T> },
}

impl<T> DictConfig<T> {
    pub fn merge(self, mut other: HashMap<String, T>) -> HashMap<String, T> {
        match self {
            DictConfig::Value(items) => items,
            DictConfig::Merge { merge } => {
                other.extend(merge);

                other
            }
            DictConfig::Replace { replace } => replace,
        }
    }
}
