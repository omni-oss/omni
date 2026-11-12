/// How a caller wants a failed write to be handled.
///
/// The storage layer reports every failure; the policy is the caller's choice
/// of what to do with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WritePolicy {
    /// A failed write is an error the caller must handle.
    Required,
    /// A failed write is logged and swallowed.
    BestEffort,
}

impl WritePolicy {
    /// Applies the policy to a write result. Returns `Err` only for
    /// [`WritePolicy::Required`].
    pub fn apply<E: std::fmt::Display>(
        self,
        result: Result<(), E>,
    ) -> Result<(), E> {
        match (self, result) {
            (_, Ok(())) => Ok(()),
            (Self::Required, Err(e)) => Err(e),
            (Self::BestEffort, Err(e)) => {
                tracing::debug!("best-effort cache write failed: {e}");
                Ok(())
            }
        }
    }
}
