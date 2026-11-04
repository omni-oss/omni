use crate::CodeInfo;

/// What one crate contributes to the registry: codes, owned prefixes, or both.
pub struct CodeRegistration {
    pub crate_name: &'static str,
    pub codes: &'static [CodeInfo],
    pub prefixes: &'static [&'static str],
}

#[cfg(all(feature = "inventory", not(feature = "explicit-registry")))]
mod backend {
    use super::CodeRegistration;

    inventory::collect!(CodeRegistration);

    pub fn registrations() -> impl Iterator<Item = &'static CodeRegistration> {
        inventory::iter::<CodeRegistration>()
    }
}

#[cfg(feature = "explicit-registry")]
mod backend {
    use std::sync::OnceLock;

    use super::CodeRegistration;

    static LIST: OnceLock<&'static [&'static CodeRegistration]> =
        OnceLock::new();

    /// Installs the registry. Call once at startup. Returns `false` if a
    /// list was already installed.
    pub fn install(list: &'static [&'static CodeRegistration]) -> bool {
        LIST.set(list).is_ok()
    }

    pub fn registrations() -> impl Iterator<Item = &'static CodeRegistration> {
        LIST.get().copied().unwrap_or(&[]).iter().copied()
    }
}

#[cfg(feature = "explicit-registry")]
pub use backend::install;
pub use backend::registrations;

/// Every registered code.
pub fn all() -> impl Iterator<Item = &'static CodeInfo> {
    registrations().flat_map(|r| r.codes.iter())
}

pub fn get(code: &str) -> Option<&'static CodeInfo> {
    all().find(|c| c.code == code)
}
