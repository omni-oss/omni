use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Jitter {
    None,
    Full,
    Equal,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Backoff {
    Fixed,
    Linear { step: Duration },
    Exponential { factor: f64 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RetryPolicy {
    pub max_retries: u8,
    pub backoff: Backoff,
    pub base: Duration,
    pub max_delay: Duration,
    pub jitter: Jitter,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 0,
            backoff: Backoff::Exponential { factor: 2.0 },
            base: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            jitter: Jitter::None,
        }
    }
}

impl RetryPolicy {
    /// Delay before retry number `retry_n` (1 for the first retry).
    ///
    /// `sample` is a uniform random number in `[0, 1)` used for jitter; it is
    /// ignored when jitter is `None`.
    pub fn delay(&self, retry_n: u8, sample: f64) -> Duration {
        let n = u32::from(retry_n.saturating_sub(1));
        let raw = match self.backoff {
            Backoff::Fixed => self.base,
            Backoff::Linear { step } => {
                self.base.saturating_add(step.saturating_mul(n))
            }
            Backoff::Exponential { .. } if self.base.is_zero() => {
                Duration::ZERO
            }
            Backoff::Exponential { factor } => Duration::try_from_secs_f64(
                self.base.as_secs_f64() * factor.powi(n as i32),
            )
            .unwrap_or(Duration::MAX),
        };
        let capped = raw.min(self.max_delay);
        let sample = sample.clamp(0.0, 1.0);
        match self.jitter {
            Jitter::None => capped,
            Jitter::Full => capped.mul_f64(sample),
            Jitter::Equal => capped / 2 + (capped / 2).mul_f64(sample),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    fn delays(p: &RetryPolicy, n: u8) -> Vec<Duration> {
        (1..=n).map(|i| p.delay(i, 0.5)).collect()
    }

    #[test]
    fn default_exponential_stops_at_max_delay() {
        let p = RetryPolicy::default();
        assert_eq!(
            delays(&p, 8),
            [1, 2, 4, 8, 16, 32, 60, 60].map(secs).to_vec()
        );
    }

    #[test]
    fn fixed_is_constant() {
        let p = RetryPolicy {
            backoff: Backoff::Fixed,
            base: secs(3),
            ..Default::default()
        };
        assert_eq!(delays(&p, 3), [3, 3, 3].map(secs).to_vec());
    }

    #[test]
    fn linear_adds_step() {
        let p = RetryPolicy {
            backoff: Backoff::Linear { step: secs(2) },
            ..Default::default()
        };
        assert_eq!(delays(&p, 3), [1, 3, 5].map(secs).to_vec());
    }

    #[test]
    fn retry_zero_is_treated_as_first() {
        let p = RetryPolicy::default();
        assert_eq!(p.delay(0, 0.0), secs(1));
    }

    #[test]
    fn huge_factor_does_not_panic() {
        let p = RetryPolicy {
            backoff: Backoff::Exponential { factor: 10.0 },
            ..Default::default()
        };
        assert_eq!(p.delay(255, 0.0), secs(60));
    }

    #[test]
    fn zero_base_stays_zero_when_exponent_overflows() {
        let p = RetryPolicy {
            backoff: Backoff::Exponential { factor: 100.0 },
            base: Duration::ZERO,
            ..Default::default()
        };
        assert_eq!(p.delay(255, 0.0), Duration::ZERO);
    }

    #[test]
    fn linear_saturates() {
        let p = RetryPolicy {
            backoff: Backoff::Linear {
                step: Duration::MAX,
            },
            ..Default::default()
        };
        assert_eq!(p.delay(255, 0.0), secs(60));
    }

    #[test]
    fn jitter_bounds() {
        let mut p = RetryPolicy {
            base: secs(10),
            backoff: Backoff::Fixed,
            jitter: Jitter::Full,
            ..Default::default()
        };
        assert_eq!(p.delay(1, 0.0), Duration::ZERO);
        assert_eq!(p.delay(1, 0.5), secs(5));
        p.jitter = Jitter::Equal;
        assert_eq!(p.delay(1, 0.0), secs(5));
        assert_eq!(p.delay(1, 1.0), secs(10));
        p.jitter = Jitter::None;
        assert_eq!(p.delay(1, 0.3), secs(10));
    }
}
