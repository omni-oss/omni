pub use ahash as hash;

pub type Map<K, V> = indexmap::IndexMap<K, V, ahash::RandomState>;
pub type UnorderedMap<K, V> =
    std::collections::HashMap<K, V, ahash::RandomState>;

pub type OrderedMap<K, V> = std::collections::BTreeMap<K, V>;

pub mod map {
    pub use indexmap::map::*;
}

pub mod unordered_map {
    pub use std::collections::hash_map::*;
}

pub mod ordered_map {
    pub use std::collections::btree_map::*;
}

#[macro_export]
macro_rules! map {
    () => {
        {
            let map = $crate::Map::with_hasher($crate::hash::RandomState::default());
            map
        }
    };
    (cap: $cap:expr $(,)?) => {
        {
            let map = $crate::Map::with_capacity_and_hasher($cap, $crate::hash::RandomState::default());
            map
        }
    };
    ($($key:expr => $value:expr,)+) => { $crate::map!($($key => $value),+) };
    ($($key:expr => $value:expr),*) => {
        {
            // Note: `stringify!($key)` is just here to consume the repetition,
            // but we throw away that string literal during constant evaluation.
            const CAP: usize = <[()]>::len(&[$({ stringify!($key); }),*]);
            let mut map = $crate::Map::with_capacity_and_hasher(CAP, $crate::hash::RandomState::default());
            $(
                map.insert($key, $value);
            )*
            map
        }
    };
}

#[macro_export]
macro_rules! unordered_map {
    () => {
        {
            let map = $crate::UnorderedMap::with_hasher($crate::hash::RandomState::default());
            map
        }
    };
    (cap: $cap:expr $(,)?) => {
        {
            let map = $crate::UnorderedMap::with_capacity_and_hasher($cap, $crate::hash::RandomState::default());
            map
        }
    };
    ($($key:expr => $value:expr,)+) => { $crate::unordered_map!($($key => $value),+) };
    ($($key:expr => $value:expr),*) => {
        {
            // Note: `stringify!($key)` is just here to consume the repetition,
            // but we throw away that string literal during constant evaluation.
            const CAP: usize = <[()]>::len(&[$({ stringify!($key); }),*]);
            let mut map = $crate::UnorderedMap::with_capacity_and_hasher(CAP, $crate::hash::RandomState::default());
            $(
                map.insert($key, $value);
            )*
            map
        }
    };
}

#[macro_export]
macro_rules! ordered_map {
    () => {
        {
            let map = $crate::OrderedMap::new();
            map
        }
    };
    ($($key:expr => $value:expr,)+) => { $crate::ordered_map!($($key => $value),+) };
    ($($key:expr => $value:expr),*) => {
        {
            let mut map = $crate::OrderedMap::new();
            $(
                map.insert($key, $value);
            )*
            map
        }
    };
}

#[cfg(feature = "concurrent")]
pub type ConcurrentMap<K, V> = dashmap::DashMap<K, V, ahash::RandomState>;
