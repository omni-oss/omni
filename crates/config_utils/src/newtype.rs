#[macro_export]
macro_rules! newtype_generic {
    ($type:ident, $inner:ident $(,)?) => {
        #[derive(
            Debug,
            Default,
            Clone,
            Copy,
            PartialEq,
            Eq,
            Hash,
            PartialOrd,
            Ord,
            derive_more::Deref,
            derive_more::From,
            derive_more::Constructor,
        )]
        #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
        #[cfg_attr(
            feature = "serde",
            derive(serde::Serialize, serde::Deserialize)
        )]
        #[cfg_attr(feature = "serde", serde(transparent))]
        #[repr(transparent)]
        pub struct $type<$inner>($inner);

        impl<$inner: Clone> $crate::traits::ToInner for $type<$inner> {
            type Inner = $inner;

            #[inline(always)]
            fn to_inner(&self) -> Self::Inner {
                self.0.clone()
            }
        }

        impl<$inner> $crate::traits::AsInner for $type<$inner> {
            type Inner = $inner;

            #[inline(always)]
            fn as_inner(&self) -> &Self::Inner {
                &self.0
            }
        }

        impl<$inner> $crate::traits::AsInnerMut for $type<$inner> {
            type Inner = $inner;

            #[inline(always)]
            fn as_inner_mut(&mut self) -> &mut Self::Inner {
                &mut self.0
            }
        }

        impl<$inner> $crate::traits::IntoInner for $type<$inner> {
            type Inner = $inner;

            #[inline(always)]
            fn into_inner(self) -> Self::Inner {
                self.0
            }
        }
    };
}

#[macro_export]
macro_rules! newtype {
    ($type:ident, $inner:ty) => {
        #[derive(
            Debug,
            Default,
            Clone,
            Copy,
            PartialEq,
            Eq,
            Hash,
            PartialOrd,
            Ord,
            derive_more::Deref,
            derive_more::From,
            derive_more::Constructor,
        )]
        #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
        #[cfg_attr(
            feature = "serde",
            derive(serde::Serialize, serde::Deserialize)
        )]
        #[cfg_attr(feature = "serde", serde(transparent))]
        #[repr(transparent)]
        pub struct $type($inner);

        impl $crate::traits::ToInner for $type {
            type Inner = $inner;

            #[inline(always)]
            pub fn to_inner(&self) -> $inner {
                self.0
            }
        }

        impl $crate::traits::AsInner for $type {
            type Inner = $inner;

            #[inline(always)]
            fn as_inner(&self) -> &Self::Inner {
                &self.0
            }
        }

        impl $crate::traits::AsInnerMut for $type {
            type Inner = $inner;

            #[inline(always)]
            fn as_inner_mut(&mut self) -> &mut Self::Inner {
                &mut self.0
            }
        }

        impl $crate::traits::IntoInner for $type {
            type Inner = $inner;

            #[inline(always)]
            fn into_inner(self) -> Self::Inner {
                self.0
            }
        }
    };
}
