pub use ahash as hash;

pub type Map<K, V> = indexmap::IndexMap<K, V, ahash::RandomState>;

pub mod map {
    pub use indexmap::map::*;
}

#[macro_export]
macro_rules! map {
    () => {
        {
            let mut map = $crate::Map::with_hasher($crate::hash::RandomState::default());
            map
        }
    };
    (cap: $cap:expr $(,)?) => {
        {
            let mut map = $crate::Map::with_capacity_and_hasher($cap, $crate::hash::RandomState::default());
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

#[cfg(feature = "concurrent")]
pub type ConcurrentMap<K, V> = dashmap::DashMap<K, V, ahash::RandomState>;
