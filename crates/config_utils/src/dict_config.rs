use std::collections::HashSet;

use maps::Map;
use strum::{EnumDiscriminants, EnumIs, IntoDiscriminant as _};

use crate::{AsInner, AsInnerMut, IntoInner, ToInner, merge::Merge};

#[derive(Debug, Clone, PartialEq, Eq, EnumIs, EnumDiscriminants)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(untagged))]
pub enum DictConfig<T: Merge> {
    Value(Map<String, T>),
    Merge { merge: Map<String, T> },
    Replace { replace: Map<String, T> },
}

impl<T: Merge> Default for DictConfig<T> {
    #[inline(always)]
    fn default() -> Self {
        Self::Value(maps::map![])
    }
}

#[cfg(feature = "validator-garde")]
impl<T: Merge + garde::Validate> garde::Validate for DictConfig<T> {
    type Context = T::Context;

    fn validate_into(
        &self,
        ctx: &Self::Context,
        mut parent: &mut dyn FnMut() -> garde::Path,
        report: &mut garde::Report,
    ) {
        let hm = self.as_map();

        for (key, value) in hm {
            let mut path = garde::util::nested_path!(parent, key);
            value.validate_into(ctx, &mut path, report);
        }
    }
}

impl<T: Merge> DictConfig<T> {
    #[inline(always)]
    pub fn value(value: Map<String, T>) -> Self {
        Self::Value(value)
    }

    #[inline(always)]
    pub fn merge(merge: Map<String, T>) -> Self {
        Self::Merge { merge }
    }

    #[inline(always)]
    pub fn replace(replace: Map<String, T>) -> Self {
        Self::Replace { replace }
    }
}

impl<T: Merge> DictConfig<T> {
    #[inline(always)]
    pub fn as_map(&self) -> &Map<String, T> {
        match self {
            DictConfig::Value(items) => items,
            DictConfig::Merge { merge } => merge,
            DictConfig::Replace { replace } => replace,
        }
    }

    #[inline(always)]
    pub fn as_map_mut(&mut self) -> &mut Map<String, T> {
        match self {
            DictConfig::Value(items) => items,
            DictConfig::Merge { merge } => merge,
            DictConfig::Replace { replace } => replace,
        }
    }

    #[inline(always)]
    pub fn into_map(self) -> Map<String, T> {
        match self {
            DictConfig::Value(items) => items,
            DictConfig::Merge { merge } => merge,
            DictConfig::Replace { replace } => replace,
        }
    }

    #[inline(always)]
    pub fn to_map(&self) -> Map<String, T>
    where
        T: Clone,
    {
        self.as_map().clone()
    }
}

impl<TInner, TWrapper: ToInner<Inner = TInner> + Merge> DictConfig<TWrapper> {
    #[inline(always)]
    pub fn to_map_inner(&self) -> Map<String, TInner> {
        self.as_map()
            .iter()
            .map(|(k, v)| (k.clone(), v.to_inner()))
            .collect()
    }
}

impl<TInner, TWrapper: IntoInner<Inner = TInner> + Merge> DictConfig<TWrapper> {
    #[inline(always)]
    pub fn into_map_inner(self) -> Map<String, TInner> {
        self.into_map()
            .into_iter()
            .map(|(k, v)| (k, v.into_inner()))
            .collect()
    }
}

impl<TInner, TWrapper: AsInner<Inner = TInner> + Merge> DictConfig<TWrapper> {
    #[inline(always)]
    pub fn to_map_as_inner(&self) -> Map<String, &TInner> {
        self.as_map()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_inner()))
            .collect()
    }
}

impl<TInner, TWrapper: AsInnerMut<Inner = TInner> + Merge>
    DictConfig<TWrapper>
{
    #[inline(always)]
    pub fn to_map_as_inner_mut(&mut self) -> Map<String, &mut TInner> {
        self.as_map_mut()
            .iter_mut()
            .map(|(k, v)| (k.clone(), v.as_inner_mut()))
            .collect()
    }
}

impl<TInner, TWrapper: ToInner<Inner = TInner> + Merge> DictConfig<TWrapper> {
    #[inline(always)]
    pub fn to_map_to_inner(&self) -> Map<String, TInner> {
        self.as_map()
            .iter()
            .map(|(k, v)| (k.clone(), v.to_inner()))
            .collect()
    }
}

impl<T: Merge> Merge for DictConfig<T> {
    fn merge(&mut self, mut other: Self) {
        // copy the discriminant of the other value
        let other_discriminant = other.discriminant();

        // swap the two values to get the discriminant of the other value
        std::mem::swap(self, &mut other);
        // swap the internal values back to the original values
        std::mem::swap(self.as_map_mut(), other.as_map_mut());

        let mut other_hash_map = other.into_map();

        match other_discriminant {
            DictConfigDiscriminants::Replace => {
                *self.as_map_mut() = other_hash_map;
            }

            DictConfigDiscriminants::Value | DictConfigDiscriminants::Merge => {
                let mut keys = HashSet::new();
                keys.extend(self.as_map().keys().cloned());
                keys.extend(other_hash_map.keys().cloned());

                for key in keys {
                    let a = self.as_map_mut().get_mut(&key);
                    let b = other_hash_map.shift_remove(&key);

                    match (a, b) {
                        (None, None) | (Some(_), None) => {
                            // do nothing
                        }
                        (None, Some(new)) => {
                            self.as_map_mut().insert(key, new);
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
    type IntoIter = maps::map::IntoIter<String, T>;

    fn into_iter(self) -> Self::IntoIter {
        match self {
            DictConfig::Value(v) => v.into_iter(),
            DictConfig::Merge { merge } => merge.into_iter(),
            DictConfig::Replace { replace } => replace.into_iter(),
        }
    }
}
impl<T: Merge> DictConfig<T> {
    pub fn iter(&'_ self) -> maps::map::Iter<'_, String, T> {
        match self {
            DictConfig::Value(v) => v.iter(),
            DictConfig::Merge { merge } => merge.iter(),
            DictConfig::Replace { replace } => replace.iter(),
        }
    }

    pub fn values(&'_ self) -> maps::map::Values<'_, String, T> {
        match self {
            DictConfig::Value(v) => v.values(),
            DictConfig::Merge { merge } => merge.values(),
            DictConfig::Replace { replace } => replace.values(),
        }
    }

    pub fn keys(&'_ self) -> maps::map::Keys<'_, String, T> {
        match self {
            DictConfig::Value(v) => v.keys(),
            DictConfig::Merge { merge } => merge.keys(),
            DictConfig::Replace { replace } => replace.keys(),
        }
    }

    pub fn iter_mut(&'_ mut self) -> maps::map::IterMut<'_, String, T> {
        match self {
            DictConfig::Value(v) => v.iter_mut(),
            DictConfig::Merge { merge } => merge.iter_mut(),
            DictConfig::Replace { replace } => replace.iter_mut(),
        }
    }

    pub fn values_mut(&'_ mut self) -> maps::map::ValuesMut<'_, String, T> {
        match self {
            DictConfig::Value(v) => v.values_mut(),
            DictConfig::Merge { merge } => merge.values_mut(),
            DictConfig::Replace { replace } => replace.values_mut(),
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

    macro_rules! map {
        [
            $($key:expr => $value:expr),*$(,)?
        ] => {
            maps::map!(
                $($key.to_string() => $value),*
            )
        };
    }

    macro_rules! map_replace {
        [
            $($key:expr => $value:expr),*$(,)?
        ] => {
            map![$($key => replace($value)),*]
        };
    }

    #[test]
    fn test_replace() {
        let mut a = DictConfig::value(map_replace![
            "foo" => 3,
            "bar" => 2
        ]);
        let b = DictConfig::replace(map_replace!("foo" => 1));

        a.merge(b);

        assert_eq!(
            a.to_map_inner(),
            map![
                "foo" => 1
            ]
        );
    }

    #[test]
    fn test_merge() {
        let mut a = DictConfig::value(map_replace![
            "foo" => 3,
            "bar" => 2
        ]);
        let b = DictConfig::merge(map_replace!("foo" => 1));

        a.merge(b);

        assert_eq!(
            a.to_map_inner(),
            map![
                "foo" => 1,
                "bar" => 2
            ]
        );
    }

    #[test]
    fn test_value() {
        // same as merge behavior
        let mut a = DictConfig::value(map_replace![
            "foo" => 3,
            "bar" => 2
        ]);
        let b = DictConfig::value(map_replace!("foo" => 1));

        a.merge(b);

        assert_eq!(
            a.to_map_inner(),
            map![
                "foo" => 1,
                "bar" => 2
            ]
        );
    }

    #[test]
    fn test_copy_other_discriminant() {
        let mut a = DictConfig::value(map_replace!["a" => 1]);
        let b = DictConfig::merge(map_replace![]);
        let c = DictConfig::value(map_replace![]);
        let d = DictConfig::replace(map_replace!["a" => 1]);

        a.merge(b);
        assert_eq!(a.discriminant(), DictConfigDiscriminants::Merge);
        a.merge(c);
        assert_eq!(a.discriminant(), DictConfigDiscriminants::Value);
        a.merge(d);
        assert_eq!(a.discriminant(), DictConfigDiscriminants::Replace);
    }
}
