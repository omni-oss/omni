use std::collections::{HashMap, HashSet, hash_map};

use strum::EnumIs;

use crate::{AsInner, AsInnerMut, IntoInner, ToInner, merge::Merge};

#[derive(Debug, Clone, PartialEq, Eq, EnumIs)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(untagged))]
pub enum DictConfig<T: Merge> {
    Value(HashMap<String, T>),
    Merge { merge: HashMap<String, T> },
    Replace { replace: HashMap<String, T> },
}

impl<T: Merge> Default for DictConfig<T> {
    #[inline(always)]
    fn default() -> Self {
        Self::Value(HashMap::new())
    }
}

impl<T: Merge> DictConfig<T> {
    #[inline(always)]
    pub fn value(value: HashMap<String, T>) -> Self {
        Self::Value(value)
    }

    #[inline(always)]
    pub fn merge(merge: HashMap<String, T>) -> Self {
        Self::Merge { merge }
    }

    #[inline(always)]
    pub fn replace(replace: HashMap<String, T>) -> Self {
        Self::Replace { replace }
    }
}

impl<T: Merge> DictConfig<T> {
    #[inline(always)]
    pub fn as_hash_map(&self) -> &HashMap<String, T> {
        match self {
            DictConfig::Value(items) => items,
            DictConfig::Merge { merge } => merge,
            DictConfig::Replace { replace } => replace,
        }
    }

    #[inline(always)]
    pub fn as_hash_map_mut(&mut self) -> &mut HashMap<String, T> {
        match self {
            DictConfig::Value(items) => items,
            DictConfig::Merge { merge } => merge,
            DictConfig::Replace { replace } => replace,
        }
    }

    #[inline(always)]
    pub fn into_hash_map(self) -> HashMap<String, T> {
        match self {
            DictConfig::Value(items) => items,
            DictConfig::Merge { merge } => merge,
            DictConfig::Replace { replace } => replace,
        }
    }

    #[inline(always)]
    pub fn to_hash_map(&self) -> HashMap<String, T>
    where
        T: Clone,
    {
        self.as_hash_map().clone()
    }
}

impl<TInner, TWrapper: ToInner<Inner = TInner> + Merge> DictConfig<TWrapper> {
    #[inline(always)]
    pub fn to_hash_map_inner(&self) -> HashMap<String, TInner> {
        self.as_hash_map()
            .iter()
            .map(|(k, v)| (k.clone(), v.to_inner()))
            .collect()
    }
}

impl<TInner, TWrapper: IntoInner<Inner = TInner> + Merge> DictConfig<TWrapper> {
    #[inline(always)]
    pub fn into_hash_map_inner(self) -> HashMap<String, TInner> {
        self.into_hash_map()
            .into_iter()
            .map(|(k, v)| (k, v.into_inner()))
            .collect()
    }
}

impl<TInner, TWrapper: AsInner<Inner = TInner> + Merge> DictConfig<TWrapper> {
    #[inline(always)]
    pub fn to_hash_map_as_inner(&self) -> HashMap<String, &TInner> {
        self.as_hash_map()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_inner()))
            .collect()
    }
}

impl<TInner, TWrapper: AsInnerMut<Inner = TInner> + Merge>
    DictConfig<TWrapper>
{
    #[inline(always)]
    pub fn to_hash_map_as_inner_mut(&mut self) -> HashMap<String, &mut TInner> {
        self.as_hash_map_mut()
            .iter_mut()
            .map(|(k, v)| (k.clone(), v.as_inner_mut()))
            .collect()
    }
}

impl<T: Merge> Merge for DictConfig<T> {
    fn merge(&mut self, other: Self) {
        match other {
            DictConfig::Replace { replace: hash_map } => {
                *self.as_hash_map_mut() = hash_map;
            }

            DictConfig::Value(mut merge) | DictConfig::Merge { mut merge } => {
                let mut keys = HashSet::new();
                keys.extend(self.as_hash_map().keys().cloned());
                keys.extend(merge.keys().cloned());

                for key in keys {
                    let a = self.as_hash_map_mut().get_mut(&key);
                    let b = merge.remove(&key);

                    match (a, b) {
                        (None, None) | (Some(_), None) => {
                            // do nothing
                        }
                        (None, Some(new)) => {
                            self.as_hash_map_mut().insert(key, new);
                        }
                        (Some(a), Some(b)) => {
                            a.merge(b);
                        }
                    }
                }
            }
        }
    }
}

impl<T: Merge> IntoIterator for DictConfig<T> {
    type Item = (String, T);
    type IntoIter = hash_map::IntoIter<String, T>;

    fn into_iter(self) -> Self::IntoIter {
        match self {
            DictConfig::Value(v) => v.into_iter(),
            DictConfig::Merge { merge } => merge.into_iter(),
            DictConfig::Replace { replace } => replace.into_iter(),
        }
    }
}
impl<T: Merge> DictConfig<T> {
    pub fn iter(&'_ self) -> std::collections::hash_map::Iter<'_, String, T> {
        match self {
            DictConfig::Value(v) => v.iter(),
            DictConfig::Merge { merge } => merge.iter(),
            DictConfig::Replace { replace } => replace.iter(),
        }
    }

    pub fn iter_mut(
        &'_ mut self,
    ) -> std::collections::hash_map::IterMut<'_, String, T> {
        match self {
            DictConfig::Value(v) => v.iter_mut(),
            DictConfig::Merge { merge } => merge.iter_mut(),
            DictConfig::Replace { replace } => replace.iter_mut(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Replace;

    use super::*;

    #[inline(always)]
    fn replace<T>(t: T) -> Replace<T> {
        Replace::new(t)
    }

    macro_rules! hm {
        [
            $($key:expr => $value:expr),*$(,)?
        ] => {
            HashMap::from([
                $(($key.to_string(), $value)),*
            ])
        };
    }

    macro_rules! hm_replace {
        [
            $($key:expr => $value:expr),*$(,)?
        ] => {
            hm![$($key => replace($value)),*]
        };
    }

    #[test]
    fn test_replace() {
        let mut a = DictConfig::value(hm_replace![
            "foo" => 3,
            "bar" => 2
        ]);
        let b = DictConfig::replace(hm_replace!("foo" => 1));

        a.merge(b);

        assert_eq!(
            a.to_hash_map_inner(),
            hm![
                "foo" => 1
            ]
        );
    }

    #[test]
    fn test_merge() {
        let mut a = DictConfig::value(hm_replace![
            "foo" => 3,
            "bar" => 2
        ]);
        let b = DictConfig::merge(hm_replace!("foo" => 1));

        a.merge(b);

        assert_eq!(
            a.to_hash_map_inner(),
            hm![
                "foo" => 1,
                "bar" => 2
            ]
        );
    }
}
