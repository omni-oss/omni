use crate::{Error, StoreState, Version};

#[derive(Debug, PartialEq, Eq)]
pub enum Check<T> {
    UpToDate,
    Work(T),
}

/// Ordinary access gate over an already observed state.
///
/// `Ready(target)` returns `UpToDate` without calling `cold` or touching any
/// registry, session or catalog; `RecoveryRequired` is never accepted as
/// current.
#[inline]
pub fn gate_access<T>(
    observed: StoreState,
    target: Version,
    cold: impl FnOnce(Version) -> Result<T, Error>,
) -> Result<Check<T>, Error> {
    match observed {
        StoreState::Ready(v) if v == target => Ok(Check::UpToDate),
        StoreState::Ready(v) => cold(v).map(Check::Work),
        StoreState::RecoveryRequired => Err(Error::Dirty),
    }
}
