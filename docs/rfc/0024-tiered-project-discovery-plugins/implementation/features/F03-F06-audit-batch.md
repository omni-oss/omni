# F03.1/F06.1 prerequisite audit batch

## Goal and scope

Complete the two existing audit tasks before scoped-reference contracts and the
effective semantic digest are implemented. This is a medium-sized,
Medium-risk documentation-only batch: it inventories behavior and implementation
gates without changing schemas, parser behavior, storage or cache keys.

- F03.1: [identity and parser inventory](F03.1-identity-parser-audit.md).
- F06.1: [effective-field inventory](F06.1-effective-field-audit.md).

The audits are independent. The integrating agent owns F03.1 and tracking;
a general-purpose agent owns the bounded F06.1 inventory. Integration checks
and review stay with the integrating agent.

## Checks and undo

Verify named code paths against the recorded baseline; distinguish observed
behavior from required future changes. Check Markdown formatting and local
links, inspect the combined diff, and obtain independent review. No Rust
runtime checks are required for documents alone. Standard PR CI remains enabled.

Undo by reverting this documentation batch; no migration or data recovery is
needed. Runtime F03/F06 features remain unimplemented.

## Progress

- F03.1 and F06.1 written and checked against the baseline.
- Independent read-only review found no material issues in F03.1. The integrating
  agent checked F06.1 against collector, context, execution, cache transport,
  platform and expression-registration code.
- Combined oxfmt and `git diff --check` pass; all 49 checked local Markdown
  links resolve. No runtime tests were run for this docs-only batch.
- PR review/CI follow. The existing schedule-generator/formatter drift noted in
  the first batch is unchanged; this batch edits only task ownership metadata,
  not dependencies, sizes or generated waves.
- F01/F02 merged in [PR #101](https://github.com/omni-oss/omni/pull/101).
- Next code slices: F03.2 shared parser/schema, F06.2 canonical digest; final
  digest integration still needs F05.1 origin tiers as specified by `tasks.yaml`.
- Follow the established workflow: PR, watch, valid review fixes, CI and
  CodeRabbit green, merge without bypassing repository requirements.
