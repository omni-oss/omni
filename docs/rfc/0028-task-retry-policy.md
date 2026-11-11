# RFC 0028: Task retry policy (exponential backoff by default)

- **Status:** Draft <!-- Draft | Accepted | Implemented | Superseded -->
- **Owner:** TBD
- **Date:** 2026-11-11
- **Base branch:** `main`
- **Affected crates/files:** `omni_core` (`RetryPolicy`, `Backoff`, `Jitter`, delay math; `core_types.rs`, `task_execution_graph.rs`); `omni_configurations` (`task_configuration.rs`, new `retry` block replacing `max_retries` and `retry_interval`); `omni_execution_plan` (`execution_plan_provider.rs`); `omni_task_executor` (`config.rs`, `pipeline.rs`, `batch_executor.rs`); `omni_cli_core` (`commands/common_args.rs`); `omni_api` (`operations/task.rs`); `omni_mcp_core` (`model/task.rs`, `tools/task.rs`); JSON schema; `apps/site` (`run` and `exec` references, task configuration reference, release note)
- **Requirements:** [0028-task-retry-policy](../requirements/0028-task-retry-policy.md)

## 1. Summary

Replace the two flat retry settings (`max_retries`, `retry_interval`) with one
structured **retry policy**. The policy has a count, a timing strategy
(`fixed`, `linear`, `exponential`), a base delay, an optional maximum delay, and optional
jitter. The default strategy is exponential backoff.

The policy is configured in full in `task_configuration`, and is available
through the API and MCP using the same structure. The CLI cannot express a
nested structure well, so it gets one compact override flag,
`-r, --retry <SPEC>`, plus `--no-retry`. The old `--retry-interval` flag is
removed. All entry points produce the same `RetryConfiguration`, which is merged
per field and resolved into a `RetryPolicy` before execution.

## 2. Motivation and current behavior

Today a task retries up to `max_retries` times, waiting a fixed
`retry_interval` between attempts. When no interval is set, the retry is
immediate. This is wired as two independent `Option` values through
`omni_configurations`, `omni_core`, `omni_execution_plan` and
`omni_task_executor`, and into `run_process` in `batch_executor.rs`:

```rust
if (result.is_err() || !success) && tries <= max_retries {
    if let Some(duration) = retry_duration && !duration.is_zero() {
        tokio::time::sleep(duration).await;
    }
    // emit TaskRetryingEvent { delay: retry_duration, .. }
    continue;
}
```

Problems:

- A fixed or zero delay is wrong for the usual reason to retry (a busy or
  rate-limited resource). It hammers the resource instead of backing off.
- Timing cannot be extended without adding another flat key per knob, and each
  knob would need its own CLI flag, config key and API field.
- The `--retry` help text refers to `max_retry`, but the config key is
  `max_retries`.

Retry settings are not part of any cache or hash input. The hashers only take
`retry_exec` (`task_retry_exec` in `omni_context/project_hasher.rs` and
`workspace_hasher.rs`). Changing the timing settings therefore does not affect
cache keys.

## 3. Goals and boundaries

Goals:

- One structured `retry` policy with `fixed`, `linear` and `exponential`
  strategies, a maximum delay and jitter. Exponential is the default strategy.
- The same structure in task configuration, the API and MCP.
- A compact CLI override that covers the common cases without a second
  structured entry point.
- Per-field merge between layers: built-in default, task config, then CLI.
- Delay computation that is pure, deterministic under a seeded RNG, and cannot
  overflow or panic.

Boundaries (out of scope):

- `retry_exec`. It is hashed, so moving it changes cache keys, which is High
  risk by the repository override. It stays a top-level task key.
- Which failures are retried (exit codes, error kinds).
- A custom list of per-attempt delays.
- Retry behavior outside task execution (`bridge_rpc_runner`,
  `omni_cache`, `appcontainer_sandbox`).

## 4. Design

### 4.1 Resolved policy (`omni_core`)

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Jitter { None, Full, Equal }

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
    /// `retry_n` is 1 for the first retry.
    pub fn delay(&self, retry_n: u8, rng: &mut impl Rng) -> Duration {
        let n = u32::from(retry_n.saturating_sub(1));
        let raw = match self.backoff {
            Backoff::Fixed => self.base,
            Backoff::Linear { step } => {
                self.base.saturating_add(step.saturating_mul(n))
            }
            Backoff::Exponential { .. } if self.base.is_zero() => Duration::ZERO,
            Backoff::Exponential { factor } => {
                Duration::try_from_secs_f64(
                    self.base.as_secs_f64() * factor.powi(n as i32),
                )
                .unwrap_or(Duration::MAX)
            }
        };
        let capped = raw.min(self.max_delay);
        match self.jitter {
            Jitter::None => capped,
            Jitter::Full => capped.mul_f64(rng.random::<f64>()),
            Jitter::Equal => {
                capped / 2 + (capped / 2).mul_f64(rng.random::<f64>())
            }
        }
    }
}
```

The RNG is injected so tests can fix it. Overflow is handled with
`try_from_secs_f64` and saturating operations.

The default `max_retries` is 0, so the default strategy only matters once a task asks
for retries.

### 4.2 Configuration (`omni_configurations`)

The configuration is one flat struct that only uses derives. It follows the
existing `Option<Replace<T>>` and `merge::option::recurse` pattern, so layers
merge per field.

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RetryStrategy { Fixed, Linear, Exponential }

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Jitter { None, Full, Equal }

/// A duration written as text, such as `500ms` or `2s`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[schemars(with = "String")]
pub struct ConfigDuration(#[serde(with = "humantime_serde")] pub Duration);

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, Merge, Validate)]
#[serde(deny_unknown_fields)]
#[garde(custom(validate_layer))]
pub struct RetryConfiguration {
    /// Maximum number of retries after the first attempt. Defaults to 0.
    pub max_retries: Option<Replace<u8>>,
    /// Backoff strategy. Defaults to `exponential`.
    pub strategy: Option<Replace<RetryStrategy>>,
    /// Delay before the first retry. Defaults to `1s`.
    pub base: Option<Replace<ConfigDuration>>,
    /// Upper bound for any single delay. Defaults to `60s`.
    pub max_delay: Option<Replace<ConfigDuration>>,
    /// Randomization applied to each delay. Defaults to `none`.
    pub jitter: Option<Replace<Jitter>>,
    /// Added to the delay after every retry. `linear` only.
    pub step: Option<Replace<ConfigDuration>>,
    /// Multiplies the delay after every retry. `exponential` only.
    pub factor: Option<Replace<f64>>,
}

// in TaskConfigurationLongForm, replacing max_retries and retry_interval:
#[serde(default, skip_serializing_if = "Option::is_none")]
pub retry: Option<RetryConfiguration>,
```

The wire format is flat. Everything is derived: serde, the JSON schema and the
merge. There is no manual `Deserialize`, no `flatten`, and no hand-written schema
or merge code.

Rules:

- **`ConfigDuration`** replaces the hand-written `mod retry_interval` in
  `task_configuration.rs`. It uses `humantime_serde`, which the workspace already
  depends on, and is the one place duration text is parsed.
- **Validate each layer on its own** (`validate_layer`, one function). Only when
  a layer names a `strategy` itself, `step` needs `linear` and `factor` needs
  `exponential`. A `step` or `factor` in a layer with no strategy is accepted,
  because the effective strategy may come from another layer; it is checked or
  ignored when resolving (next rule). So `-r 255,factor=10` is valid. `deny_unknown_fields` rejects unknown keys and
  typos, and serde rejects a bad value or an unknown strategy.
- **Ignore inapplicable fields when resolving.** After the layers are merged, a
  `step` (from any layer, including a strategy-less `-r 3,step=1s`) is ignored if the effective strategy is not
  `linear`, and likewise `factor`. This is what makes `-r 3,fixed` work on a task
  whose config says `strategy: linear, step: 2s`, with no merge code that has to
  know which fields belong together.
- **Resolve into the typed policy.** `into_policy(self) -> Result<RetryPolicy, _>`
  fills the defaults, builds the strict `Backoff` enum from 4.1, and checks the
  values serde cannot: `factor` is finite and `>= 1.0`, and `max_delay >= base`. A missing `step` defaults to `base`, so `-r 3,linear` works.
  There is no "uncapped" value: `max_delay` always has a value (default 60s), and
  a user who wants a very long cap sets a large one such as `max_delay: 24h`. The type-level guarantee (a `Linear` policy has a `step`, an
  `Exponential` policy has a `factor`) lives in `RetryPolicy`, which the executor
  uses.

Example:

```yaml
tasks:
  fetch:
    command: curl ...
    retry:
      max_retries: 5
      strategy: exponential
      base: 500ms
      factor: 2
      max_delay: 30s
      jitter: full
```

The flat `max_retries` and `retry_interval` keys of the task are removed. A config
that still uses them fails to load as an unknown field.

**Why not a tagged enum on the wire.** A tagged enum (`#[serde(tag = "strategy")]`)
would make an invalid `step` or `factor` unwritable and give a `oneOf` schema,
but the tag has to be optional so that a layer can set only some fields. A
prototype of `#[serde(flatten)] backoff: Option<Backoff>` swallowed every error
inside the flattened enum and produced `None`:

| Input | Result |
| --- | --- |
| `{"max":3,"strategy":"expontential"}` | `backoff: None`, no error |
| `{"max":3,"strategy":"linear","step":"abc"}` | `backoff: None`, no error |
| `{"max":3,"factor":3.0}` | `backoff: None`, `factor` ignored |
| `{"max":3,"strategy":"fixed","factor":3.0}` | `Fixed`, `factor` ignored |

Making the result safe needs a manual `Deserialize` (a raw struct plus
`TryFrom`), a hand-written schema, and a merge rule for the enum, which is three
pieces to keep in step with the fields. The flat struct avoids all of them, and
the strict enum is kept where it matters, in `RetryPolicy`.

### 4.3 CLI (`omni_cli_core/common_args.rs`)

```rust
#[arg(short = 'r', long, value_name = "SPEC",
      help = "Retry policy: COUNT[,strategy][,key=value...], e.g. 3,exponential,base=500ms,max_delay=30s,jitter=full",
      value_parser = parse_retry_spec)]
pub retry: Option<RetryConfiguration>,

#[arg(long, conflicts_with = "retry", help = "Disable retries")]
pub no_retry: bool,
```

The parser only splits the text. It builds a JSON object and lets the
configuration's own deserializer do the rest, so the spec accepts exactly the
keys and values the config accepts, with the same errors, and adding a field to
`RetryConfiguration` needs no parser change.

```rust
fn parse_retry_spec(s: &str) -> Result<RetryConfiguration, String> {
    let mut parts = s.split(',');
    let mut obj = serde_json::Map::new();
    let count = parts.next().unwrap_or_default();
    obj.insert("max_retries".into(), scalar(count));
    for p in parts {
        let (k, v) = p.split_once('=').unwrap_or(("strategy", p));
        if obj.insert(k.into(), scalar(v)).is_some() {
            return Err(format!("`{k}` given more than once"));
        }
    }
    let cfg: RetryConfiguration =
        serde_json::from_value(obj.into()).map_err(|e| e.to_string())?;
    cfg.validate().map_err(|e| e.to_string())?;
    Ok(cfg)
}

// `3` becomes a number, `500ms` and `full` stay strings.
fn scalar(v: &str) -> serde_json::Value {
    serde_json::from_str(v).unwrap_or_else(|_| v.into())
}
```

Grammar:

- The first item is the retry count, an integer.
- A bare word is the strategy.
- The remaining items are `key=value`, using the same names as the config.
- Unknown keys, a bad value, and a `step` or `factor` that does not fit the
  strategy are errors from the same code that checks the config.

`--no-retry` sets `max_retries = 0`. `--retry-interval` is removed outright, with
no hidden stub or deprecation message. Passing it gets clap's standard
unknown-argument error.

There is deliberately no `--retry-config` or per-field flags. Anything beyond the
spec's grammar is set in the task configuration.

### 4.4 Precedence and merge

From lowest to highest: built-in default, task configuration, `-r`. Merging is per
field, so `-r 5` changes only `max_retries`, and `-r 3,jitter=none` changes the
count and the jitter and keeps the rest. Each layer is validated on its own
before the merge (4.2), and fields that do not apply to the effective strategy
are ignored afterwards, so `-r 3,fixed` on a task configured with `linear` and
`step` works and leaves no stray `step`.

The API and MCP take a `RetryConfiguration` and use the same merge, so all
entry points resolve identically.

### 4.5 Executor (`omni_task_executor/batch_executor.rs`)

`run_process` takes `policy: &RetryPolicy` in place of `max_retries` and
`retry_duration`.

```rust
if failed && tries <= policy.max_retries {
    let delay = policy.delay(tries, &mut rand::rng());
    subscriber.on_task_retrying(TaskRetryingEvent {
        /* ... */
        attempt: tries,
        max_retries: policy.max_retries,
        delay: Some(delay),
    }).await;
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
    continue;
}
```

`TaskRetryingEvent.delay` now reports the delay actually used for that attempt,
and the event is emitted before the sleep, so a subscriber can show "retrying in
4s". Before this change the event was emitted after the sleep. The release note
mentions the new order.

### 4.6 Worked example (test fixture)

| Input | Expected delays |
| --- | --- |
| `-r 4` (defaults) | 1s, 2s, 4s, 8s |
| `-r 8` | 1s, 2s, 4s, 8s, 16s, 32s, 60s, 60s (`max_delay`) |
| `-r 3,fixed,base=2s` | 2s, 2s, 2s |
| `-r 3,linear,base=1s,step=2s` | 1s, 3s, 5s |
| `-r 3,exponential,base=1s,jitter=full` | each in `[0, d]`, `d` the un-jittered delay |
| config `retry: {max_retries: 5, jitter: full}` plus `-r 3` | `max_retries` 3, jitter full kept |
| `-r 3,step=1s` (exponential) | `step` ignored, delays 1s, 2s, 4s |
| `--retry-interval 2s` | clap unknown-argument error |
| `-r 255,factor=10` | no panic, capped by `max_delay` |
| no `max_retries` anywhere | no retries |

## 5. Test plan

- **`omni_core`:** unit tests for every row of 4.6 on `RetryPolicy::delay`,
  including overflow, `max_retries = 0`, `retry_n = 0`, and jitter bounds with a seeded
  RNG.
- **`omni_configurations`:** deserialization of the `retry` block (plain
  scalars through `Replace`), an unknown key, an unknown strategy and a bad
  duration each being an error, per-layer validation of `step` and `factor`,
  `into_policy()` errors, per-field merge across layers, a leftover `step` or
  `factor` being ignored after a strategy switch, and rejection of the removed
  flat keys.
- **`omni_cli_core`:** a table test of `parse_retry_spec` (every valid form and
  every error, and a new `RetryConfiguration` field being accepted with no parser
  change), `--no-retry` conflicting with `-r`, that `--retry-interval` is no longer accepted,
  and a `--help` snapshot.
- **`omni_task_executor`:** a retry-loop test with `base = 0` that checks the
  attempt count and the `delay` carried by `TaskRetryingEvent`.
- **`omni_api`:** a request carrying a `RetryConfiguration` produces the same
  policy as the equivalent CLI spec.

Checks per touched crate: `cargo clippy -p <crate> -- -D warnings`,
`cargo fmt -p <crate> -- --check`, `cargo test -p <crate>`. Whole-workspace runs
are left to CI.

## 6. Compatibility and rollout

This is a breaking change to the CLI and the config format, and it changes the
default timing. It is High risk.

- **Default timing.** A task with `max_retries` and no `retry_interval` used to
  retry immediately. It now waits 1s, 2s, 4s and so on. Release note and docs
  state this and show how to restore the old behavior
  (`retry: {max_retries: N, strategy: fixed, base: 0s}`).
- **Removed config keys.** The task-level `max_retries` and `retry_interval` fail
  to load through the existing `deny_unknown_fields` on the task configuration
  (serde's "unknown field" error). There is no custom message and no alias.
- **Removed flag.** `--retry-interval` is deleted from the code, and clap rejects
  it as an unknown argument. The release note lists the replacement
  (`-r COUNT,base=DUR`).
  `-r 3` keeps working as before for the count.
- **Cache.** No cache key changes: retry timing is not hashed, and `retry_exec`
  is untouched.

Steps. Each leaves the tree working.

1. **Policy type and delay math** (`omni_core`). New code that nothing calls yet.
   Undo: revert the commit.
2. **Config, plumbing and executor, together.** The signature change and its
   callers go together. Add `RetryConfiguration` (with `ConfigDuration`, per-layer validation and
   `into_policy()`),
   remove the flat keys, and thread `RetryPolicy` through `omni_core`,
   `omni_execution_plan` and the executor. The CLI temporarily maps the old flags
   onto a `RetryConfiguration`. Undo: revert the commit.
3. **CLI swap.** Add the `-r SPEC` parser and `--no-retry`, and delete
   `--retry-interval` and its plumbing.
4. **API, MCP, schema and docs.** Switch `omni_api`'s `TaskRunFilters` (which
   today has `retry: Option<u8>` and `retry_interval: Option<Duration>`) to
   carry `retry: Option<RetryConfiguration>`. MCP's `TaskRunParams`
   (`omni_mcp_core/src/model/task.rs`) has no retry field today and builds the
   filters with `..Default::default()`, so add an optional
   `retry: Option<RetryConfiguration>` there and pass it through in
   `tools/task.rs` (for both the task run and the exec command). Regenerate the
   JSON schema, update `apps/site`, and write the release note.

The riskiest part is the contract and the delay math, so steps 1 and 2 come
first.

Undo after release: fix forward by changing `RetryPolicy::default()`. A bad
default is a one-line change; a wrong config key name is harder to undo, which is
why the names are settled here.

## 7. Alternatives and review questions

Alternatives considered:

- **Keep the flat keys and add more** (`retry_strategy`, `retry_factor`, ...).
  Rejected: every knob needs a key, a flag and an API field, and the keys can
  contradict each other.
- **One structured `--retry-config` flag on the CLI** (inline JSON or YAML, or
  `@file`). Rejected: the structured form already lives in the task config and
  the MCP input schema, so this would be a second entry point for the same thing.
- **Separate flags per field** (`--retry-factor`, `--retry-jitter`, ...).
  Rejected: clutters `--help`, and has an unclear precedence between them.
- **Keep `--retry-interval` as an alias for the base delay.** Rejected: removing
  it is allowed and avoids two ways to say the same thing.
- **Tagged enum on the wire** (`#[serde(tag = "strategy")]`, as in `ToolBackend`
  and `ActionConfiguration`). Rejected: with the tag optional it swallows errors
  (4.2), and making it safe needs a manual `Deserialize`, a hand-written schema
  and a merge rule. With the tag required, a layer cannot set only some fields,
  which breaks per-field merge and `-r 5`.
- **Nested `backoff:` object holding the tagged enum.** Rejected: fully derived
  and safe, but the CLI parser would have to know which keys nest, which brings
  back per-field parser code.
- **Move `retry_exec` into the `retry` block.** Rejected for now: it is hashed, so
  moving it changes cache keys (High risk by override).

Decisions taken in review:

1. Defaults: `base` 1s, `factor` 2, `max_delay` 60s, `jitter` none. The `max_delay` mainly stops
   a large retry count from waiting for hours.
2. The retry event is emitted before the sleep (see 4.5).
3. The discriminator is `strategy`, which has precedent in user-facing config
   (`omni_projection_configurations`). `type` (`ActionConfiguration`,
   `ToolBackend`) selects a kind of object, not a mode.
4. MCP has no retry options today, so this adds a new optional `retry` parameter
   (step 4).
5. The config is one flat derived struct (4.2). Optional flatten of a tagged enum
   swallows errors, and the safe alternatives need manual serde code.
   Hand-written serde, schema and merge code is avoided; the CLI parser reuses the
   config's deserializer.
6. The count and the maximum delay are `max_retries` and `max_delay`.

Open questions: none.

## 8. Implementation status and deviations

Steps 1 to 4 are implemented. Deviations from the design above:

- **New crate `omni_retry_config`.** `RetryConfiguration`, `ConfigDuration`,
  `RetryPolicy`, `Backoff` and `Jitter` live here, not in `omni_core` (policy) or
  `omni_configurations` (config). `omni_configurations` depends on `omni_core`,
  and `omni_core`'s `Task` and `TaskExecutionNode` have to carry the retry
  settings, so the config type cannot live in `omni_configurations`. A small
  crate below both avoids the cycle and keeps `omni_core` free of config code.
- **Jitter input.** `RetryPolicy::delay(retry_n, sample)` takes a uniform sample
  in `[0, 1)` instead of an `Rng`. The executor passes `rand::random()`. Tests
  need no seeded RNG and `omni_retry_config` has no `rand` dependency.
- **Validation without garde.** `RetryConfiguration::check_layer()` is a plain
  method. `TaskConfigurationLongForm` calls it through a field-level
  `garde(custom(...))`, and the CLI parser calls it directly. A struct-level
  `garde(custom)` was not usable with `allow_unvalidated`.
- **Equality.** `RetryConfiguration` implements `Eq` and `Ord` by hand (comparing
  `factor` by its bits) because `TaskExecutionNode` derives them and `factor` is
  an `f64` that is validated finite.
- **Resolution point.** The task's configuration and the run-level override are
  merged and resolved into a `RetryPolicy` at the start of `run_process`, so an
  invalid combination fails that task with an error message.
- **Single CLI step.** The temporary mapping of the old flags was skipped; the
  new flags replaced them in the same change.
- **Repository retries removed for now.** Seven `project.omni.yaml` files used
  the removed task keys. CI runs a released omni that rejects `retry:`, and the new
  omni rejects the old keys, so this change drops their retry settings so that both
  load. Restore `retry:` in those files in a follow-up once a release containing
  this change is used by CI.
- **Docs.** The `run` and `exec` references were edited by hand, because
  regenerating them also rewrote unrelated pages. A new guide,
  `apps/site/content/docs/guides/retrying-tasks.mdx`, covers the settings and the
  migration.
- **Not done.** A retry-loop test in `omni_task_executor` with `base = 0`; the
  delay math and the config are unit-tested instead.
