use derive_new::new;
use strum::{EnumIs, EnumTryAs};

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, EnumIs, EnumTryAs, new,
)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", serde(untagged))]
#[cfg_attr(feature = "schemars", schemars(untagged))]
pub enum SingleOrMany<T> {
    Single(T),
    Many(Vec<T>),
}

impl<T> SingleOrMany<T> {
    pub fn into_vec(self) -> Vec<T> {
        match self {
            SingleOrMany::Single(item) => vec![item],
            SingleOrMany::Many(items) => items,
        }
    }

    pub fn as_slice(&self) -> Option<&[T]> {
        match self {
            SingleOrMany::Single(_) => None,
            SingleOrMany::Many(items) => Some(items.as_slice()),
        }
    }
}

impl<T: Clone> SingleOrMany<T> {
    pub fn to_vec(&self) -> Vec<T> {
        match self {
            SingleOrMany::Single(item) => vec![item.clone()],
            SingleOrMany::Many(items) => items.clone(),
        }
    }
}

impl<T> From<T> for SingleOrMany<T> {
    fn from(value: T) -> Self {
        Self::Single(value)
    }
}

impl<T> From<Vec<T>> for SingleOrMany<T> {
    fn from(value: Vec<T>) -> Self {
        Self::Many(value)
    }
}

impl<T: Default> Default for SingleOrMany<T> {
    fn default() -> Self {
        Self::Single(T::default())
    }
}

#[cfg(feature = "merge")]
impl<T> merge::Merge for SingleOrMany<T> {
    fn merge(&mut self, other: Self) {
        *self = other;
    }
}
