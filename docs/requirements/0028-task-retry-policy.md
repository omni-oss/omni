# Requirements: Task retry policy

- **RFC:** [0028-task-retry-policy](../rfc/0028-task-retry-policy.md)
- **Status:** Draft
- **Date:** 2026-11-11

## Summary

A failed task retries up to `max_retries` times with a fixed `retry_interval`, or
immediately when none is set. This change replaces both settings with one
structured retry policy: a count, a timing strategy (`fixed`, `linear`,
`exponential`), a base delay, a maximum delay and jitter. Exponential backoff is the
default strategy. The full policy is set in the task configuration, the API and
MCP. The CLI gets one compact override flag and a flag to turn retries off.

## Goals

- Retry timing is configurable as `fixed`, `linear` or `exponential`, with a
  base delay, an optional maximum delay and optional jitter.
- The default strategy is exponential backoff, with a maximum on any single delay.
- Task configuration, the API and MCP use the same structure.
- The CLI can override the count and the common timing options in one flag,
  `-r, --retry <SPEC>`.
- `--no-retry` turns retries off whatever the configuration says.
- Layers merge per field: built-in default, then task configuration, then CLI.
- Delay computation cannot overflow or panic for any input.

## Non-goals

- Changing `retry_exec`, which is part of the task hash.
- Choosing which failures are retried.
- Custom per-attempt delay lists.
- Retry behavior outside task execution.

## Acceptance criteria

1. With defaults and `-r 4`, the delays before the retries are 1s, 2s, 4s, 8s.
2. With defaults and `-r 8`, delays stop growing at the 60s `max_delay`.
3. `-r 3,fixed,base=2s` waits 2s before each retry.
4. `-r 3,linear,base=1s,step=2s` waits 1s, 3s, 5s.
5. With `jitter=full`, each delay is between 0 and the un-jittered delay. With
   `jitter=equal`, it is between half the delay and the full delay.
6. A task configured with `retry: {max_retries: 5, jitter: full}` run with `-r 3` retries
   3 times with full jitter.
7. Naming a strategy in `-r` drops `factor` and `step` set in lower layers.
8. `-r 3,step=1s` on an exponential policy fails with an error saying `step` is
   only valid for `linear`. An unknown key fails with an error listing the valid
   keys.
9. `-r 255,factor=10` does not panic and respects `max_delay`.
10. `--no-retry` together with `-r` is rejected. `--no-retry` alone means no
    retries.
11. `--retry-interval` no longer exists. Passing it is rejected by clap as an
    unknown argument, and no code for it remains.
12. A task configuration that uses the old task-level `max_retries` or
    `retry_interval` keys fails to load as an unknown field.
13. The retry event reports the delay used for that attempt.
14. Task cache keys are unchanged by this change.
15. The JSON schema, the `run` and `exec` references, the task configuration
    reference and a release note are updated. The release note states the new
    default timing and shows how to get the old immediate retry back.
