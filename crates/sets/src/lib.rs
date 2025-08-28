pub use ahash as hash;

pub type Set<K> = indexmap::IndexSet<K, ahash::RandomState>;

pub mod set {
    pub use indexmap::set::*;
}

pub type UnorderedSet<K> = std::collections::HashSet<K, ahash::RandomState>;

pub mod unordered_set {
    pub use std::collections::hash_set::*;
}

pub type OrderedSet<K> = std::collections::BTreeSet<K>;

pub mod ordered_set {
    pub use std::collections::btree_set::*;
}

#[macro_export]
macro_rules! set {
    () => {
        {
            let set = $crate::Set::with_hasher($crate::hash::RandomState::default());
            set
        }
    };
    (cap: $cap:expr $(,)?) => {
        {
            let set = $crate::Set::with_capacity_and_hasher($cap, $crate::hash::RandomState::default());
            set
        }
    };
    ($($value:expr,)+) => { $crate::set!($($value),+) };
    ($($value:expr),*) => {{
        const CAP: usize = <[()]>::len(&[$({ stringify!($value); }),*]);
        let mut set = Set::with_capacity_and_hasher(CAP, ahash::RandomState::default());
        $(
            set.insert($value);
        )*
        set
    }};
}

#[macro_export]
macro_rules! unordered_set {
    () => {
        {
            let set = $crate::UnorderedSet::with_hasher($crate::hash::RandomState::default());
            set
        }
    };
    (cap: $cap:expr $(,)?) => {
        {
            let set = $crate::UnorderedSet::with_capacity_and_hasher($cap, $crate::hash::RandomState::default());
            set
        }
    };
    ($($value:expr,)+) => { $crate::unordered_set!($($value),+) };
    ($($value:expr),*) => {{
        const CAP: usize = <[()]>::len(&[$({ stringify!($value); }),*]);
        let mut set = UnorderedSet::with_capacity_and_hasher(CAP, ahash::RandomState::default());
        $(
            set.insert($value);
        )*
        set
    }};
}

#[macro_export]
macro_rules! ordered_set {
    () => {
        {
            let set = $crate::OrderedSet::new();
            set
        }
    };
    ($($value:expr,)+) => { $crate::ordered_set!($($value),+) };
    ($($value:expr),*) => {{
        let mut set = $crate::OrderedSet::new();
        $(
            set.insert($value);
        )*
        set
    }};
}

#[cfg(feature = "concurrent")]
pub type ConcurrentSet<K> = dashmap::DashSet<K, ahash::RandomState>;
