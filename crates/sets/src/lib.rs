pub use ahash as hash;

pub type Set<K> = indexmap::IndexSet<K, ahash::RandomState>;

pub mod set {
    pub use indexmap::set::*;
}

#[macro_export]
macro_rules! set {
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

#[cfg(feature = "concurrent")]
pub type ConcurrentSet<K> = dashmap::DashSet<K, ahash::RandomState>;
