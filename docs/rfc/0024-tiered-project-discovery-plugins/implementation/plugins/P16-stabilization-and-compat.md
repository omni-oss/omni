# P16: Stabilization, compatibility and migrations

**Size:** L · **Depends on:** P15

## Goal

Freeze the stable contract by omni 1.0 and ship the compatibility and migration
machinery that keeps it stable.

## RFC references

- [§14 versioning, compatibility and migrations](../../014.00-versioning-compatibility-and-migrations.md) (esp. 14.4-14.6)
- [§10 step 5](../../010.00-implementation-and-rollout.md)

## Work

1. Freeze protocol/capability 1.0 types, WIT package and Rust SDK surface; semver policy
   and support/deprecation window documented.
2. Stable old-artifact fixtures run against every host update for all supported majors.
3. Negotiation tests (ranges, missing features, overclaims) in the release gate.
4. SDK source-compatibility checks within a stable SDK major.
5. Migration tooling: previews, adjacent-version composition, idempotence, stale-plan
   detection, backups, interrupted multi-file recovery, manual-refusal paths; test the
   unversioned legacy workspace baseline.
6. Author docs: plugin guide, statelessness responsibilities, conformance testing,
   toolchain pinning; user docs for activation, `apply`, `defaults`, conflicts.
7. Keep activation explicit after stabilization.

## Acceptance

- Release checklist from §14.6 automated in CI.
- Published benchmarks and docs; RFC status moved to Accepted/Implemented with
  deviations recorded.
