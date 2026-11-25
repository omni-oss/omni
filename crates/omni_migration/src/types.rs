#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Version(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StoreId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Revision(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CatalogId(pub u32);

/// Names the chain a store belongs to, so a store is never run against another
/// store's catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChainName(&'static str);

impl ChainName {
    pub const fn new(name: &'static str) -> Self {
        Self(name)
    }

    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

/// A chain name and the digest of the validated chain it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChainBinding {
    pub name: ChainName,
    pub digest: [u8; 32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreState {
    Ready(Version),
    RecoveryRequired,
}

/// A coherent observation of one store state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub store: StoreId,
    pub chain: ChainName,
    pub revision: Revision,
    pub version: Version,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepCapabilities {
    pub atomic: bool,
    pub lossless: bool,
    pub derivable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackendSupport {
    pub durable: bool,
    pub atomic_publish: bool,
}

impl BackendSupport {
    pub const FULL: Self = Self {
        durable: true,
        atomic_publish: true,
    };
    pub const DURABLE_NON_ATOMIC: Self = Self {
        durable: true,
        atomic_publish: false,
    };
    pub const NONE: Self = Self {
        durable: false,
        atomic_publish: false,
    };
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Policy {
    pub allow_non_atomic: bool,
    pub allow_derivable_loss: bool,
    pub allow_irreversible_loss: bool,
}

impl Policy {
    pub fn check(self, c: StepCapabilities) -> Result<(), crate::Error> {
        use crate::Error;
        if !c.atomic && !self.allow_non_atomic {
            return Err(Error::NonAtomic);
        }
        match (c.lossless, c.derivable) {
            (true, _) => Ok(()),
            (false, true) if self.allow_derivable_loss => Ok(()),
            (false, false) if self.allow_irreversible_loss => Ok(()),
            (false, true) => Err(Error::DerivableLoss),
            (false, false) => Err(Error::IrreversibleLoss),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Upgrade,
    Downgrade,
}
