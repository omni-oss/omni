use crate::{AsInner, AsInnerMut, IntoInner, ToInner, merge::Merge};
use strum::EnumIs;

#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, EnumIs)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(untagged))]
pub enum ListConfig<T: Merge> {
    Value(Vec<T>),
    Merge { merge: Vec<T> },
    Append { append: Vec<T> },
    Prepend { prepend: Vec<T> },
    Replace { replace: Vec<T> },
}

impl<T: Merge> Default for ListConfig<T> {
    fn default() -> Self {
        Self::Value(Vec::new())
    }
}

impl<T: Merge> ListConfig<T> {
    #[inline(always)]
    pub fn value(value: Vec<T>) -> Self {
        Self::Value(value)
    }

    #[inline(always)]
    pub fn merge(merge: Vec<T>) -> Self {
        Self::Merge { merge }
    }

    #[inline(always)]
    pub fn append(append: Vec<T>) -> Self {
        Self::Append { append }
    }

    #[inline(always)]
    pub fn prepend(prepend: Vec<T>) -> Self {
        Self::Prepend { prepend }
    }

    #[inline(always)]
    pub fn replace(replace: Vec<T>) -> Self {
        Self::Replace { replace }
    }
}

impl<T: Merge> ListConfig<T> {
    pub fn as_vec(&self) -> &Vec<T> {
        match self {
            ListConfig::Value(items) => items,
            ListConfig::Merge { merge } => merge,
            ListConfig::Replace { replace } => replace,
            ListConfig::Append { append } => append,
            ListConfig::Prepend { prepend } => prepend,
        }
    }

    pub fn as_vec_mut(&mut self) -> &mut Vec<T> {
        match self {
            ListConfig::Value(items) => items,
            ListConfig::Merge { merge } => merge,
            ListConfig::Replace { replace } => replace,
            ListConfig::Append { append } => append,
            ListConfig::Prepend { prepend } => prepend,
        }
    }

    pub fn into_vec(self) -> Vec<T> {
        match self {
            ListConfig::Value(items) => items,
            ListConfig::Merge { merge } => merge,
            ListConfig::Replace { replace } => replace,
            ListConfig::Append { append } => append,
            ListConfig::Prepend { prepend } => prepend,
        }
    }

    pub fn to_vec(&self) -> Vec<T>
    where
        T: Clone,
    {
        self.as_vec().clone()
    }
}

impl<TInner, TWrapper: ToInner<Inner = TInner> + Merge> ListConfig<TWrapper> {
    pub fn to_vec_inner(&self) -> Vec<TInner> {
        self.as_vec()
            .iter()
            .map(|wrapper| wrapper.to_inner())
            .collect()
    }
}

impl<TInner, TWrapper: IntoInner<Inner = TInner> + Merge> ListConfig<TWrapper> {
    pub fn into_vec_inner(self) -> Vec<TInner> {
        self.into_vec()
            .into_iter()
            .map(|wrapper| wrapper.into_inner())
            .collect()
    }
}

impl<TInner, TWrapper: AsInner<Inner = TInner> + Merge> ListConfig<TWrapper> {
    pub fn to_vec_as_inner(&self) -> Vec<&TInner> {
        self.as_vec()
            .iter()
            .map(|wrapper| wrapper.as_inner())
            .collect()
    }
}

impl<TInner, TWrapper: AsInnerMut<Inner = TInner> + Merge>
    ListConfig<TWrapper>
{
    pub fn to_vec_as_inner_mut(&mut self) -> Vec<&mut TInner> {
        self.as_vec_mut()
            .iter_mut()
            .map(|wrapper| wrapper.as_inner_mut())
            .collect()
    }
}

impl<TInner, TWrapper: ToInner<Inner = TInner> + Merge> ListConfig<TWrapper> {
    pub fn to_vec_to_inner(&self) -> Vec<TInner> {
        self.as_vec()
            .iter()
            .map(|wrapper| wrapper.to_inner())
            .collect()
    }
}

impl<T: Merge> Merge for ListConfig<T> {
    fn merge(&mut self, other: Self) {
        match other {
            ListConfig::Value(items)
            | ListConfig::Replace { replace: items } => {
                *self.as_vec_mut() = items;
            }
            ListConfig::Merge { merge } => {
                let self_items = self.as_vec_mut().iter_mut();

                for (a, b) in self_items.zip(merge) {
                    a.merge(b);
                }
            }
            ListConfig::Append { mut append } => {
                self.as_vec_mut().append(&mut append);
            }
            ListConfig::Prepend { mut prepend } => {
                let current = self.as_vec_mut();
                std::mem::swap(current, &mut prepend);
                self.as_vec_mut().append(&mut prepend);
            }
        }
    }
}

impl<T: Merge> IntoIterator for ListConfig<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        match self {
            ListConfig::Value(v) => v.into_iter(),
            ListConfig::Merge { merge } => merge.into_iter(),
            ListConfig::Append { append } => append.into_iter(),
            ListConfig::Prepend { prepend } => prepend.into_iter(),
            ListConfig::Replace { replace } => replace.into_iter(),
        }
    }
}

impl<T: Merge> ListConfig<T> {
    pub fn iter(&'_ self) -> std::slice::Iter<'_, T> {
        match self {
            ListConfig::Value(v) => v.iter(),
            ListConfig::Merge { merge } => merge.iter(),
            ListConfig::Append { append } => append.iter(),
            ListConfig::Prepend { prepend } => prepend.iter(),
            ListConfig::Replace { replace } => replace.iter(),
        }
    }

    pub fn iter_mut(&'_ mut self) -> std::slice::IterMut<'_, T> {
        match self {
            ListConfig::Value(v) => v.iter_mut(),
            ListConfig::Merge { merge } => merge.iter_mut(),
            ListConfig::Append { append } => append.iter_mut(),
            ListConfig::Prepend { prepend } => prepend.iter_mut(),
            ListConfig::Replace { replace } => replace.iter_mut(),
        }
    }
}

#[cfg(feature = "validator-garde")]
impl<T: Merge + garde::Validate> garde::Validate for ListConfig<T> {
    type Context = T::Context;

    fn validate_into(
        &self,
        ctx: &Self::Context,
        mut parent: &mut dyn FnMut() -> garde::Path,
        report: &mut garde::Report,
    ) {
        let vec = self.as_vec();

        for (idx, value) in vec.iter().enumerate() {
            let mut path = garde::util::nested_path!(parent, idx);
            value.validate_into(ctx, &mut path, report);
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

    macro_rules! vec_replace {
        [$($t:expr),*$(,)?] => {
            vec![$(replace($t)),*]
        };
    }

    #[test]
    fn test_value() {
        let mut a = ListConfig::value(vec_replace![1, 2, 3]);
        let b = ListConfig::value(vec_replace![4, 5, 6]);

        a.merge(b);

        assert_eq!(a.to_vec_inner(), vec![4, 5, 6]);
    }

    #[test]
    fn test_prepend() {
        let mut a = ListConfig::value(vec_replace![1, 2, 3]);
        let b = ListConfig::append(vec_replace![4, 5, 6]);

        a.merge(b);

        assert_eq!(a.to_vec_inner(), vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn test_append() {
        let mut a = ListConfig::value(vec_replace![4, 5, 6]);
        let b = ListConfig::prepend(vec_replace![1, 2, 3]);

        a.merge(b);

        assert_eq!(a.to_vec_inner(), vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn test_replace() {
        let mut a = ListConfig::replace(vec_replace![1, 2, 3]);
        let b = ListConfig::value(vec_replace![4, 5, 6]);

        a.merge(b);

        assert_eq!(a.to_vec_inner(), vec![4, 5, 6]);
    }

    #[test]
    fn test_merge() {
        let mut a = ListConfig::merge(vec_replace![1, 2, 3]);
        let b = ListConfig::merge(vec_replace![4, 5]);

        a.merge(b);

        assert_eq!(a.to_vec_inner(), vec![4, 5, 3]);
    }
}
