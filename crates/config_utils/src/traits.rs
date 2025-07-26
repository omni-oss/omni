pub trait MergeExt: merge::Merge {
    #[inline(always)]
    fn cloned_merge(&mut self, other: &mut Self) -> Self
    where
        Self: Clone,
    {
        let mut this = self.clone();
        this.merge(other.clone());

        this
    }
}

pub trait ToInner {
    type Inner;

    fn to_inner(&self) -> Self::Inner;
}

pub trait AsInner {
    type Inner;

    fn as_inner(&self) -> &Self::Inner;
}

pub trait AsInnerMut {
    type Inner;

    fn as_inner_mut(&mut self) -> &mut Self::Inner;
}

pub trait IntoInner {
    type Inner;

    fn into_inner(self) -> Self::Inner;
}
