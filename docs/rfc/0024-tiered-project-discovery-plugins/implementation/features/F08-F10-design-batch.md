# F08.1/F10.1 design batch

## Goal and scope

Land the two remaining independent design documents that gate the shared-infrastructure
code slices, before any of that code is written. Medium risk, documentation only: no
code, schema, store layout, lockfile format, cache key or capability policy changes.

- F08.1: [lease, GC and reference protocol](F08.1-lease-gc-reference-protocol.md). Gates
  F08.2-F08.5.
- F10.1: [captured input view design](F10.1-captured-input-view-design.md). Gates
  F10.2-F10.3. It touches a High-risk area (capabilities) as a design only; the code
  slices need the High-risk review the document calls for.

The two are independent. A separate writer drafted each; the integrating agent checked
their load-bearing claims against the code and owns tracking and the combined result.

## Checks and undo

Verify "observed" claims against the baseline symbols, keep observed and proposed
separate, check formatting and local links, and obtain an independent read-only review.
No Rust checks apply to documents alone; standard PR CI stays enabled. Undo by reverting
this documentation batch; nothing to migrate.

## Progress

- F01/F02 merged in [PR #101](https://github.com/omni-oss/omni/pull/101); F03.1/F06.1
  merged in [PR #103](https://github.com/omni-oss/omni/pull/103).
- F08.1 and F10.1 drafted; independent read-only review done.
- Review corrections applied to F08.1: generator/tool callers write their full declared
  set (not a subset), so only a future partial caller needs `merge_additive`; and the
  proposed atomic temp-file write cannot reuse `omni_file_data_serde::write_async`
  unchanged because it infers the format from the extension. F10.1: no findings.
- PR, CI and CodeRabbit review follow.
- Open decisions are owned in each document (F08.1 D1-D8, F10.1 D1-D10). The ones that
  most affect the code slices: store `VERSION` bump (F08.1 D1), unlinking lease files
  (F08.1 D2), and symlink policy plus macOS no-follow primitives (F10.1 D2, D5).
- Next code slices that become ready: F08.2 (lease primitive) and F10.2 (read/exists/
  list). F03.2 (parser/schema) and F06.2 (canonical digest) are ready independently.
