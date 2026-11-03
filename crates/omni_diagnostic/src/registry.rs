use std::error::Error;

use crate::{CodeInfo, Diagnostic};

/// One registered diagnostic type.
pub struct Registration {
    pub type_name: &'static str,
    pub codes: &'static [CodeInfo],
    pub lookup:
        for<'a> fn(&'a (dyn Error + 'static)) -> Option<&'a dyn Diagnostic>,
}

#[cfg(all(feature = "inventory", not(feature = "explicit-registry")))]
mod backend {
    use super::Registration;

    inventory::collect!(Registration);

    pub fn registrations() -> impl Iterator<Item = &'static Registration> {
        inventory::iter::<Registration>()
    }
}

#[cfg(feature = "explicit-registry")]
mod backend {
    use std::sync::OnceLock;

    use super::Registration;

    static LIST: OnceLock<&'static [&'static Registration]> = OnceLock::new();

    /// Installs the registry. Call once at startup. Returns `false` if a
    /// list was already installed.
    pub fn install(list: &'static [&'static Registration]) -> bool {
        LIST.set(list).is_ok()
    }

    pub fn registrations() -> impl Iterator<Item = &'static Registration> {
        LIST.get().copied().unwrap_or(&[]).iter().copied()
    }
}

#[cfg(feature = "explicit-registry")]
pub use backend::install;
pub use backend::registrations;
