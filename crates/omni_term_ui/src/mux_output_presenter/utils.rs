use std::sync::atomic::{AtomicU64, Ordering};

use maps::UnorderedMap;
use tokio::task::JoinHandle;

/// A presenter-unique handle for one registered stream. It routes joins,
/// removal, screens, scroll state and keyboard input. The string id given to
/// `add_stream` is only a title and may repeat; it is never a registry key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StreamId(u64);

#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("the presenter has no stream handles left")]
pub struct StreamIdExhausted;

/// Allocates monotonic [`StreamId`]s, failing instead of wrapping.
#[derive(Debug, Default)]
pub struct StreamIdAllocator {
    next: AtomicU64,
}

impl StreamIdAllocator {
    pub fn allocate(&self) -> Result<StreamId, StreamIdExhausted> {
        let mut current = self.next.load(Ordering::Relaxed);
        loop {
            let next = current.checked_add(1).ok_or(StreamIdExhausted)?;
            match self.next.compare_exchange_weak(
                current,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Ok(StreamId(current)),
                Err(actual) => current = actual,
            }
        }
    }

    #[cfg(test)]
    fn starting_at(n: u64) -> Self {
        Self {
            next: AtomicU64::new(n),
        }
    }
}

pub type TasksMap<E> = UnorderedMap<StreamId, JoinHandle<Result<(), E>>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_monotonic() {
        let ids = StreamIdAllocator::default();
        let a = ids.allocate().unwrap();
        let b = ids.allocate().unwrap();
        assert!(a < b);
        assert_ne!(a, b);
    }

    #[test]
    fn exhaustion_is_an_error_not_a_wrap() {
        let ids = StreamIdAllocator::starting_at(u64::MAX - 1);
        assert!(ids.allocate().is_ok());
        assert!(ids.allocate().is_err());
        assert!(ids.allocate().is_err());
    }
}
