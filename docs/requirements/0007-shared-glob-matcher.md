# Requirements: Shared glob matcher with include/exclude negation

- **RFC:** [0007-shared-glob-matcher](../rfc/0007-shared-glob-matcher.md)
- **Status:** Accepted
- **Date:** 2026-08-29

## Summary

omni already lets authors write include/exclude glob lists with a leading-`!` convention, but only some subsystems understand it. This change makes that one matcher a shared, reusable capability with the same negation rule everywhere, adds the anchoring and separator flexibility needed for more callers to adopt it, and preserves the exact behavior of the subsystems that use it today. It unblocks projection routing (RFC 0005) reusing the same convention instead of inventing a second one.

## Goals

- One include/exclude glob convention shared across file-selection subsystems, not one per subsystem.
- Preserve the existing `!`-negation semantics exactly for the subsystems that already use them.
- Make the matcher flexible enough (anchoring, separator handling) that projection routing can adopt it.
- Keep glob matching deterministic and its result caching correct.

## Non-goals

- Changing the config surface of task discovery, configuration discovery, or the generator.
- Changing projection config (that is RFC 0005).
- Moving hashing/caching-related glob callers onto the shared matcher (deferred to a future RFC).
- Adopting gitignore-style ordered, first-match-wins precedence.

## Requirements

### Configuration

REQ-01 This change introduces no new user-facing config fields; the projection config surface that consumes the shared convention is specified in RFC 0005.

### Behavior

REQ-02 The include/exclude convention must be a single flat list of glob patterns in which a leading `!` marks a pattern as an exclusion.
REQ-03 A pattern's role must be decided by the parity of its leading `!` characters: zero or an even count is an include, an odd count is an exclude, so `!!keep` is an include and `!drop` is an exclude.
REQ-04 Matching must be order-independent, and exclusion must always win, so a path is selected only when at least one include matches it and no exclude matches it.
REQ-05 A list containing only exclude patterns must match nothing, so "everything except X" must be written as an explicit include plus an exclude (for example `**` then `!X`).
REQ-06 The matcher must support matching patterns as written against a path supplied by the caller (for example a path relative to a source root).
REQ-07 The matcher must support matching patterns anchored to a root directory against absolute paths.
REQ-08 The matcher must support controlling whether `*` and `?` may cross `/` (a literal-separator option).
REQ-09 Task discovery, configuration discovery, and the generator must produce identical results before and after this change, with the same anchoring and separator behavior they have today.
REQ-10 The `!`-negation convention must be reusable by projection routing so RFC 0005 can adopt it rather than defining a separate exclusion mechanism.
REQ-11 Glob matching must remain deterministic, a pure function of the patterns, the options, and the path being tested.
REQ-12 Result caching must keep different option sets distinct, so patterns compiled under one separator setting never return results computed under another.
REQ-13 The system must expose a way to classify a single raw pattern as an include or an exclude, so configuration validators can reject exclude-only lists without reimplementing the parity rule.

### Breaking changes

REQ-14 There are no user-observable breaking changes; the internal reorganization preserves the behavior and config of every affected subsystem.

## Migration / upgrade path

No action is required from users. Existing glob lists in task discovery, configuration discovery, and generator configs continue to behave exactly as before.

## Technical Details

The matcher that currently lives in the discovery-utilities module moves into a small standalone crate so it can be shared. The `!`-parity split, pattern stripping, and "include and not exclude" matching are preserved verbatim. Two options are added: relative-versus-rooted anchoring and separator handling. The existing memoized compiled-glob cache is extended so its key includes these options, and the current cache function keeps its signature and default-option key shape so existing callers are untouched. The three current consumers migrate as behavior-preserving renames, guarded by their existing test suites; the old single-module crate is then removed with no compatibility shim.

## Out of scope

- Moving the remaining raw glob callers tied to hashing and caching onto the shared matcher; deferred to a future RFC because of determinism and stale-cache risk.
- Policy path matching with fail-closed semantics, which is a separate concern.
- Any projection config field changes, which are specified in RFC 0005.
- gitignore-style ordered, first-match-wins precedence.
