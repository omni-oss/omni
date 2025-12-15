use std::{
    fmt::Display,
    process,
    sync::{
        LazyLock,
        atomic::{AtomicU32, Ordering},
    },
};

use rand::Rng;

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    Default,
)]
#[serde(transparent)]
#[repr(transparent)]
pub struct Id(u64);

impl Id {
    pub fn new() -> Self {
        Self(unique_u64_for_pid())
    }
}

impl Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

static COUNTER: LazyLock<AtomicU32> = LazyLock::new(|| {
    // seed counter with random start to reduce chance of collision after PID reuse
    let seed: u32 = rand::rng().random();
    AtomicU32::new(seed)
});

/// Returns a u64 that is unique per-call within the process and unique across
/// concurrently running processes on the same machine (PID in high 32 bits).
pub fn unique_u64_for_pid() -> u64 {
    let pid = process::id(); // u32
    let low = COUNTER.fetch_add(1, Ordering::Relaxed);
    ((pid as u64) << 32) | (low as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn unique_in_process() {
        let mut seen = HashSet::new();
        for _ in 0..1_000_000u32 {
            let v = unique_u64_for_pid();
            assert!(seen.insert(v));
        }
    }
}
