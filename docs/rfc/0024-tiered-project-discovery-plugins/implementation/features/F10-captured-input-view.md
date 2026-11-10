# F10: Captured, containment-checked input view

**Type:** feature (shared infrastructure) · **Size:** L · **Depends on:** R02

## Problem

Today file reads are checked by canonicalize-then-open, which leaves symlink-swap races,
and the task hasher's size/mtime optimization cannot prove content is unchanged. A
multi-step process that reads several files has no consistent view and no way to detect
that an input changed between steps.

## Goal

A command-local **captured input view**: reads go through one service that (a) enforces
containment at access time, (b) hashes the bytes actually supplied, (c) records tracked
inputs including **negative** reads (missing files) and directory membership, and (d)
can revalidate the whole set at the end, rejecting detected changes. Usable by any
subsystem; discovery is the first intended consumer.

## RFC references

- [§17.2](../../017.00-review-findings-and-prerequisites.md#172-frozen-claims-do-not-imply-frozen-filesystem-inputs)
- [§12.4 host services](../../012.00-rust-trait-sketches.md#124-host-services-plugin-side-view)
- [§18.3 capability evaluation row](../../018.00-readiness-and-open-decisions.md#183-implementation-gates-grounded-in-current-code)
- [§8.1 shared safeguards](../../008.00-performance-and-shared-safeguards.md#81-shared-safeguards)

## Code starting points

- `crates/omni_capability_sys/src/sys.rs` (documents residual check/open races)
- `crates/omni_capabilities/src/eval.rs` (policy evaluation, rooted matching)
- `crates/omni_capability_enforcement`
- `crates/omni_collector`, `omni_hasher`, `system_traits`

## Slices

1. Design doc: API, per-platform containment-at-access strategy, limits (bytes, depth,
   entries), tracked-evidence format using `WorkspacePath` (R02).
2. Implement read/exists/list with tracking and limits **before** allocation.
3. Revalidation: compare bytes/immutable identity, not size+mtime alone; same-size/
   same-stamp edit detection; created/deleted file and membership changes.
4. Default policy: reject on detected change. Bounded automatic restart is optional and
   off (zero retries); do not implement the retry loop here.

## Out of scope

- Plugin permission profile schema, hook host services.

## Acceptance

- Symlink swap during read cannot escape containment (platform tests where possible).
- Same-size same-mtime edit detected on revalidation.
- Missing-file read recorded as negative evidence; later creation detected.
- Limits enforced before large allocation.
- Cancellation leaves no partial state. Benchmarks for overhead on a 1,000-file tree.

## Note

If this proves too plugin-shaped on review, keep the generic read/track/revalidate core
here and leave hook-specific host services for the plugin work.
