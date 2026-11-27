//! The effective-task-config digest contribution (F06.2).
//!
//! A task's cache key must change when the task's *effective* execution
//! semantics change, whatever raw configuration text produced them. This module
//! defines a typed projection of those semantics ([`EffectiveTaskConfig`]) and a
//! versioned, canonical byte encoding of it. The digest is omni's content hasher
//! over exactly those bytes, so the encoding is the contract: any change to it
//! needs a new [`VERSION`].
//!
//! The digest reaches every cache key as the effective-task leaf the collector
//! inserts (F06.3). `omni_task_context::effective_task::build` is the one builder
//! of this value; every site that computes a key reads the digest it produces.
//!
//! # Encoding
//!
//! Every value is self-delimiting, so no two different configurations can encode
//! to the same bytes by shifting a boundary (`["ab","c"]` vs `["a","bc"]`):
//!
//! | Kind | Bytes |
//! | --- | --- |
//! | bool | one byte, `0` or `1` |
//! | count / length | `u64`, little endian |
//! | string | length, then the UTF-8 bytes |
//! | option | `0`, or `1` then the value |
//! | enum | one variant byte, then the payload |
//! | list | count, then each item in order |
//! | set | count, then each item sorted by encoded bytes, duplicates removed |
//! | map | count, then each `(key, value)` sorted by key |
//! | float | the IEEE-754 bits, with `-0.0` as `+0.0` and every NaN as one NaN |
//!
//! The stream starts with the length-framed [`VERSION`] tag, then each field as
//! `field id (u8)` followed by its value, in the fixed order of
//! [`EffectiveTaskConfig::encode`]. Lists whose order is semantic (command
//! arguments, operands) keep their order; collections that are unordered in
//! meaning (maps, dependency and path-rule sets) are canonicalized.
//!
//! # What is, and is not, in the digest
//!
//! Included: the effective definition after composition. Excluded by
//! construction (there is no field for them): description, `base`/`extends`
//! ancestry, output-log display policy, redaction, cache enablement and its
//! provenance, measured results. See `F06.1-effective-field-audit.md`.
//!
//! Not in this digest, because they belong to the invocation and not the
//! finalized definition: the merged invocation arguments (they reach the key
//! through the execution info instead), invocation retry overrides, and the
//! platform, loaded environment values and path context that command expressions
//! consume. Putting platform in this per-task digest would also stop unrelated
//! tasks sharing a remote cache across operating systems.

use std::collections::{BTreeMap, BTreeSet};

use omni_command_config::{CommandConfig, CommandOperand};
use omni_hasher::{
    Hasher,
    impls::{DefaultHash, DefaultHasher},
};
use omni_retry_config::{Backoff, Jitter, RetryPolicy};
use omni_types::Root;

/// The encoding version, also the digest's domain tag. A different encoding is a
/// different version: old and new digests must never be comparable by accident.
pub const VERSION: &str = "effective-task-config/v1";

/// An argument or metadata value, typed so the encoding does not depend on any
/// JSON serialization. An integer `1` and a float `1.0` are different values, as
/// are the string `"1"` and the integer `1`.
#[derive(Debug, Clone, PartialEq)]
pub enum EffectiveValue {
    Null,
    Bool(bool),
    Int(i64),
    /// Only for integers above [`i64::MAX`]; smaller ones are always `Int`.
    Uint(u64),
    Float(f64),
    String(String),
    List(Vec<EffectiveValue>),
    Map(BTreeMap<String, EffectiveValue>),
}

impl From<serde_json::Value> for EffectiveValue {
    fn from(value: serde_json::Value) -> Self {
        match value {
            serde_json::Value::Null => Self::Null,
            serde_json::Value::Bool(b) => Self::Bool(b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Self::Int(i)
                } else if let Some(u) = n.as_u64() {
                    Self::Uint(u)
                } else {
                    Self::Float(n.as_f64().unwrap_or(f64::NAN))
                }
            }
            serde_json::Value::String(s) => Self::String(s),
            serde_json::Value::Array(items) => {
                Self::List(items.into_iter().map(Self::from).collect())
            }
            serde_json::Value::Object(map) => Self::Map(
                map.into_iter().map(|(k, v)| (k, Self::from(v))).collect(),
            ),
        }
    }
}

/// Whether the task runs. A boolean and an expression that renders to the same
/// truth value are different definitions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Enabled {
    Bool(bool),
    Expression(String),
}

impl Default for Enabled {
    fn default() -> Self {
        Self::Bool(true)
    }
}

/// A glob rule anchored at a root. The same pattern under a different root
/// selects different files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathRule {
    pub root: Root,
    pub pattern: String,
}

/// Which files and environment values the task declares as cache inputs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InputPolicy {
    /// Whether configuration files are added as default inputs. This is the
    /// effective policy, never a switch for the digest itself: the digest is
    /// mandatory even when this is `false`.
    pub key_defaults: bool,
    pub include: Vec<PathRule>,
    pub exclude: Vec<PathRule>,
    pub env_keys: BTreeSet<String>,
}

/// Which files are preserved and replayed, and whether logs are captured.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutputPolicy {
    pub include: Vec<PathRule>,
    pub exclude: Vec<PathRule>,
    /// Whether the run captures logs so a hit can replay them. It is in the
    /// digest on purpose: an entry stored without logs cannot replay them, so a
    /// task that later asks for logs must miss rather than hit a log-less entry.
    /// This is the capture policy, not how logs are displayed.
    pub capture_logs: bool,
}

/// The execution-relevant effective definition of one finalized task.
///
/// Callers fill this from the composed task, so equal effective semantics give
/// equal values whatever configuration produced them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EffectiveTaskConfig {
    pub command: Option<CommandConfig>,
    pub retry_command: Option<CommandConfig>,
    pub enabled: Enabled,
    pub interactive: bool,
    pub persistent: bool,
    /// The resolved policy with every default filled in, so an explicit value
    /// equal to a default digests the same as leaving it out.
    pub retry: RetryPolicy,
    /// Configured arguments, not the merged invocation arguments.
    pub args: BTreeMap<String, EffectiveValue>,
    /// Workspace `env.vars` definitions, before expansion.
    pub env_workspace: BTreeMap<String, String>,
    /// Project `env.vars` definitions (composed through `extends`), before
    /// expansion. Kept apart from the workspace's: a project variable can read
    /// a key the workspace also defines and sees the project's own value of it.
    pub env_project: BTreeMap<String, String>,
    /// Task `env.vars` definitions, before expansion.
    pub env_task: BTreeMap<String, String>,
    /// Effective merged project and task metadata. Included because command
    /// expressions can read it; narrowing is unproven.
    pub meta: BTreeMap<String, EffectiveValue>,
    /// Resolved execution dependencies, by full task name.
    pub dependencies: BTreeSet<String>,
    /// Resolved co-scheduled tasks (`with`), by full task name. Not a dependency
    /// edge, and the siblings' own definitions are not included.
    pub siblings: BTreeSet<String>,
    pub input: InputPolicy,
    pub output: OutputPolicy,
}

mod field {
    pub const COMMAND: u8 = 1;
    pub const RETRY_COMMAND: u8 = 2;
    pub const ENABLED: u8 = 3;
    pub const INTERACTIVE: u8 = 4;
    pub const PERSISTENT: u8 = 5;
    pub const RETRY: u8 = 6;
    pub const ARGS: u8 = 7;
    pub const ENV_WORKSPACE: u8 = 8;
    pub const META: u8 = 9;
    pub const DEPENDENCIES: u8 = 10;
    pub const SIBLINGS: u8 = 11;
    pub const INPUT: u8 = 12;
    pub const OUTPUT: u8 = 13;
    pub const ENV_PROJECT: u8 = 14;
    pub const ENV_TASK: u8 = 15;
}

impl EffectiveTaskConfig {
    /// The canonical encoding: the bytes the digest is computed over.
    pub fn encode(&self) -> Vec<u8> {
        self.encode_versioned(VERSION)
    }

    /// The digest contribution: omni's content hasher over [`encode`](Self::encode).
    pub fn digest(&self) -> DefaultHash {
        DefaultHasher::hash(&self.encode())
    }

    fn encode_versioned(&self, version: &str) -> Vec<u8> {
        let mut e = Encoder::default();
        e.str(version);

        e.u8(field::COMMAND);
        e.option(self.command.as_ref(), Encoder::command);
        e.u8(field::RETRY_COMMAND);
        e.option(self.retry_command.as_ref(), Encoder::command);
        e.u8(field::ENABLED);
        match &self.enabled {
            Enabled::Bool(b) => {
                e.u8(0);
                e.bool(*b);
            }
            Enabled::Expression(s) => {
                e.u8(1);
                e.str(s);
            }
        }
        e.u8(field::INTERACTIVE);
        e.bool(self.interactive);
        e.u8(field::PERSISTENT);
        e.bool(self.persistent);
        e.u8(field::RETRY);
        e.retry(&self.retry);
        e.u8(field::ARGS);
        e.map(&self.args, Encoder::value);
        e.u8(field::ENV_WORKSPACE);
        e.map(&self.env_workspace, |e, s| e.str(s));
        e.u8(field::ENV_PROJECT);
        e.map(&self.env_project, |e, s| e.str(s));
        e.u8(field::ENV_TASK);
        e.map(&self.env_task, |e, s| e.str(s));
        e.u8(field::META);
        e.map(&self.meta, Encoder::value);
        e.u8(field::DEPENDENCIES);
        e.set(self.dependencies.iter(), |e, s| e.str(s));
        e.u8(field::SIBLINGS);
        e.set(self.siblings.iter(), |e, s| e.str(s));
        e.u8(field::INPUT);
        e.bool(self.input.key_defaults);
        e.set(self.input.include.iter(), Encoder::path_rule);
        e.set(self.input.exclude.iter(), Encoder::path_rule);
        e.set(self.input.env_keys.iter(), |e, s| e.str(s));
        e.u8(field::OUTPUT);
        e.set(self.output.include.iter(), Encoder::path_rule);
        e.set(self.output.exclude.iter(), Encoder::path_rule);
        e.bool(self.output.capture_logs);
        e.finish()
    }
}

#[derive(Default)]
struct Encoder {
    buf: Vec<u8>,
}

impl Encoder {
    fn finish(self) -> Vec<u8> {
        self.buf
    }

    fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    fn bool(&mut self, v: bool) {
        self.u8(u8::from(v));
    }

    fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn len(&mut self, n: usize) {
        // usize is at most 64 bits on every supported target.
        self.u64(n as u64);
    }

    fn str(&mut self, s: &str) {
        self.len(s.len());
        self.buf.extend_from_slice(s.as_bytes());
    }

    fn f64(&mut self, v: f64) {
        let bits = if v.is_nan() {
            0x7ff8_0000_0000_0000
        } else if v == 0.0 {
            0
        } else {
            v.to_bits()
        };
        self.u64(bits);
    }

    fn duration(&mut self, d: std::time::Duration) {
        self.u64(d.as_secs());
        self.buf.extend_from_slice(&d.subsec_nanos().to_le_bytes());
    }

    fn option<T>(&mut self, v: Option<T>, f: impl FnOnce(&mut Self, T)) {
        match v {
            None => self.u8(0),
            Some(v) => {
                self.u8(1);
                f(self, v);
            }
        }
    }

    fn map<V>(
        &mut self,
        map: &BTreeMap<String, V>,
        mut f: impl FnMut(&mut Self, &V),
    ) {
        // A BTreeMap iterates in key order, which is the canonical order.
        self.len(map.len());
        for (k, v) in map {
            self.str(k);
            f(self, v);
        }
    }

    /// Items are encoded, sorted by their encoded bytes and deduplicated, so the
    /// order and repetition a caller supplied never reach the digest.
    fn set<'a, T: 'a>(
        &mut self,
        items: impl Iterator<Item = &'a T>,
        mut f: impl FnMut(&mut Self, &T),
    ) {
        let mut encoded: Vec<Vec<u8>> = items
            .map(|item| {
                let mut inner = Encoder::default();
                f(&mut inner, item);
                inner.finish()
            })
            .collect();
        encoded.sort();
        encoded.dedup();
        self.len(encoded.len());
        for item in encoded {
            self.buf.extend_from_slice(&item);
        }
    }

    fn path_rule(&mut self, rule: &PathRule) {
        self.u8(match rule.root {
            Root::Workspace => 0,
            Root::Project => 1,
        });
        self.str(&rule.pattern);
    }

    fn command(&mut self, command: &CommandConfig) {
        match command {
            CommandConfig::Shell(s) => {
                self.u8(0);
                self.str(s);
            }
            CommandConfig::Argv(items) => {
                self.u8(1);
                self.len(items.len());
                for item in items {
                    self.str(item);
                }
            }
            CommandConfig::Operands(items) => {
                self.u8(2);
                self.len(items.len());
                for item in items {
                    self.operand(item);
                }
            }
        }
    }

    fn operand(&mut self, operand: &CommandOperand) {
        match operand {
            CommandOperand::Literal(s) => {
                self.u8(0);
                self.str(s);
            }
            CommandOperand::Expression(s) => {
                self.u8(1);
                self.str(s);
            }
            CommandOperand::Interpolated {
                expression,
                literals,
            } => {
                self.u8(2);
                self.str(expression);
                self.map(literals, |e, s| e.str(s));
            }
        }
    }

    fn retry(&mut self, policy: &RetryPolicy) {
        let RetryPolicy {
            max_retries,
            backoff,
            base,
            max_delay,
            jitter,
        } = policy;
        self.u8(*max_retries);
        match backoff {
            Backoff::Fixed => self.u8(0),
            Backoff::Linear { step } => {
                self.u8(1);
                self.duration(*step);
            }
            Backoff::Exponential { factor } => {
                self.u8(2);
                self.f64(*factor);
            }
        }
        self.duration(*base);
        self.duration(*max_delay);
        self.u8(match jitter {
            Jitter::None => 0,
            Jitter::Full => 1,
            Jitter::Equal => 2,
        });
    }

    fn value(&mut self, value: &EffectiveValue) {
        match value {
            EffectiveValue::Null => self.u8(0),
            EffectiveValue::Bool(b) => {
                self.u8(1);
                self.bool(*b);
            }
            EffectiveValue::Int(i) => {
                self.u8(2);
                self.buf.extend_from_slice(&i.to_le_bytes());
            }
            EffectiveValue::Uint(u) => {
                self.u8(3);
                self.u64(*u);
            }
            EffectiveValue::Float(f) => {
                self.u8(4);
                self.f64(*f);
            }
            EffectiveValue::String(s) => {
                self.u8(5);
                self.str(s);
            }
            EffectiveValue::List(items) => {
                self.u8(6);
                self.len(items.len());
                for item in items {
                    self.value(item);
                }
            }
            EffectiveValue::Map(map) => {
                self.u8(7);
                self.map(map, Encoder::value);
            }
        }
    }
}
