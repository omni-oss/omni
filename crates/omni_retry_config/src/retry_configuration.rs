use std::time::Duration;

use config_utils::{IntoInner, Replace};
use merge::Merge;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Backoff, Jitter, RetryPolicy};

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    JsonSchema,
    strum::Display,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum RetryStrategy {
    Fixed,
    Linear,
    Exponential,
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum JitterConfiguration {
    None,
    Full,
    Equal,
}

impl From<JitterConfiguration> for Jitter {
    fn from(value: JitterConfiguration) -> Self {
        match value {
            JitterConfiguration::None => Jitter::None,
            JitterConfiguration::Full => Jitter::Full,
            JitterConfiguration::Equal => Jitter::Equal,
        }
    }
}

/// A duration written as text, such as `500ms` or `2s`.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(transparent)]
#[schemars(with = "String")]
pub struct ConfigDuration(#[serde(with = "humantime_serde")] pub Duration);

#[derive(Debug, Clone, thiserror::Error, PartialEq)]
pub enum RetryConfigurationError {
    #[error("`step` is only valid for the `linear` strategy")]
    StepWithoutLinear,
    #[error("`factor` is only valid for the `exponential` strategy")]
    FactorWithoutExponential,
    #[error("`factor` must be finite and at least 1.0")]
    InvalidFactor,
    #[error("`max_delay` must not be smaller than `base`")]
    MaxDelayBelowBase,
}

/// One layer of retry settings. Layers merge per field.
#[derive(
    Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema, Merge,
)]
#[serde(deny_unknown_fields)]
pub struct RetryConfiguration {
    /// Maximum number of retries after the first attempt. Defaults to 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub max_retries: Option<Replace<u8>>,
    /// Backoff strategy. Defaults to `exponential`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub strategy: Option<Replace<RetryStrategy>>,
    /// Delay before the first retry. Defaults to `1s`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub base: Option<Replace<ConfigDuration>>,
    /// Upper bound for any single delay. Defaults to `60s`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub max_delay: Option<Replace<ConfigDuration>>,
    /// Randomization applied to each delay. Defaults to `none`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub jitter: Option<Replace<JitterConfiguration>>,
    /// Added to the delay after every retry. `linear` only. Defaults to `base`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub step: Option<Replace<ConfigDuration>>,
    /// Multiplies the delay after every retry. `exponential` only.
    /// Defaults to 2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub factor: Option<Replace<f64>>,
}

// `factor` is validated finite, so equality is reflexive.
impl Eq for RetryConfiguration {}

impl Ord for RetryConfiguration {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let key = |c: &Self| {
            (
                c.max_retries,
                c.strategy,
                c.base,
                c.max_delay,
                c.jitter,
                c.step,
                c.factor.map(|f| f.into_inner().to_bits()),
            )
        };
        key(self).cmp(&key(other))
    }
}

impl PartialOrd for RetryConfiguration {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl RetryConfiguration {
    /// Checks one layer on its own. A `step` or `factor` is only rejected
    /// when this layer names a conflicting strategy itself.
    pub fn check_layer(&self) -> Result<(), RetryConfigurationError> {
        if let Some(strategy) = self.strategy.as_ref().map(|s| **s) {
            if self.step.is_some() && strategy != RetryStrategy::Linear {
                return Err(RetryConfigurationError::StepWithoutLinear);
            }
            if self.factor.is_some() && strategy != RetryStrategy::Exponential {
                return Err(RetryConfigurationError::FactorWithoutExponential);
            }
        }
        if let Some(factor) = self.factor.as_ref().map(|f| **f)
            && !(factor.is_finite() && factor >= 1.0)
        {
            return Err(RetryConfigurationError::InvalidFactor);
        }
        Ok(())
    }

    /// Resolves merged layers into a policy, filling defaults. A `step` or
    /// `factor` that does not apply to the effective strategy is ignored.
    pub fn into_policy(self) -> Result<RetryPolicy, RetryConfigurationError> {
        if let Some(f) = self.factor.as_ref().map(|f| **f)
            && !(f.is_finite() && f >= 1.0)
        {
            return Err(RetryConfigurationError::InvalidFactor);
        }
        let d = RetryPolicy::default();
        let base = self.base.map_or(d.base, |v| v.into_inner().0);
        let max_delay =
            self.max_delay.map_or(d.max_delay, |v| v.into_inner().0);
        if max_delay < base {
            return Err(RetryConfigurationError::MaxDelayBelowBase);
        }
        let backoff = match self
            .strategy
            .map_or(RetryStrategy::Exponential, |v| v.into_inner())
        {
            RetryStrategy::Fixed => Backoff::Fixed,
            RetryStrategy::Linear => Backoff::Linear {
                step: self.step.map_or(base, |v| v.into_inner().0),
            },
            RetryStrategy::Exponential => Backoff::Exponential {
                factor: self.factor.map_or(2.0, |v| v.into_inner()),
            },
        };
        Ok(RetryPolicy {
            max_retries: self
                .max_retries
                .map_or(d.max_retries, |v| v.into_inner()),
            backoff,
            base,
            max_delay,
            jitter: self.jitter.map_or(d.jitter, |v| v.into_inner().into()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> serde_json::Result<RetryConfiguration> {
        serde_json::from_str(json)
    }

    #[test]
    fn plain_scalars_deserialize() {
        let c = parse(
            r#"{"max_retries":3,"strategy":"linear","base":"500ms","step":"2s","jitter":"full"}"#,
        )
        .unwrap();
        let p = c.into_policy().unwrap();
        assert_eq!(p.max_retries, 3);
        assert_eq!(p.base, Duration::from_millis(500));
        assert_eq!(
            p.backoff,
            Backoff::Linear {
                step: Duration::from_secs(2)
            }
        );
        assert_eq!(p.jitter, Jitter::Full);
    }

    #[test]
    fn bad_input_is_an_error() {
        assert!(parse(r#"{"strategy":"expontential"}"#).is_err());
        assert!(parse(r#"{"base":"abc"}"#).is_err());
        assert!(parse(r#"{"retry_interval":"1s"}"#).is_err());
    }

    #[test]
    fn layer_validation() {
        let c = parse(r#"{"strategy":"fixed","step":"1s"}"#).unwrap();
        assert!(c.check_layer().is_err());
        let ok = |j| parse(j).unwrap().check_layer().is_ok();
        assert!(!ok(r#"{"strategy":"fixed","factor":3}"#));
        assert!(ok(r#"{"factor":10}"#));
        assert!(!ok(r#"{"factor":0.5}"#));
    }

    #[test]
    fn defaults_and_merge() {
        assert_eq!(
            RetryConfiguration::default().into_policy().unwrap(),
            RetryPolicy::default()
        );
        let mut low =
            parse(r#"{"max_retries":5,"strategy":"linear","step":"2s","jitter":"full"}"#)
                .unwrap();
        low.merge(parse(r#"{"max_retries":3,"strategy":"fixed"}"#).unwrap());
        let p = low.into_policy().unwrap();
        assert_eq!(p.max_retries, 3);
        assert_eq!(p.backoff, Backoff::Fixed);
        assert_eq!(p.jitter, Jitter::Full);
    }

    #[test]
    fn linear_step_defaults_to_base_and_cap_checked() {
        let p = parse(r#"{"strategy":"linear","base":"3s"}"#)
            .unwrap()
            .into_policy()
            .unwrap();
        assert_eq!(
            p.backoff,
            Backoff::Linear {
                step: Duration::from_secs(3)
            }
        );
        assert_eq!(
            parse(r#"{"base":"2m"}"#).unwrap().into_policy(),
            Err(RetryConfigurationError::MaxDelayBelowBase)
        );
    }
}
