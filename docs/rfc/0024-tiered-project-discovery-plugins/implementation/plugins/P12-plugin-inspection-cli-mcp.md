# P12: Plugin inspection, notices and CLI/MCP surfaces

**Size:** M · **Depends on:** P08, F09

## Goal

Make plugin behavior observable and debuggable from the CLI and MCP, with no new
notice channel.

## RFC references

- [§9 failures, security and observability](../../009.00-failures-security-and-observability.md)
- [§13.6 definition of done](../../013.00-implementation-plan.md#136-definition-of-done-for-v1)
- RFC 0021-0023 (diagnostics, redaction, notices, tracing)

## Work

1. Inspection data: enabled activations with effective ids, artifact identity,
   negotiated versions/features, per-root dispositions, inference-cache hit/miss status,
   field provenance (F07), cache eligibility (F09), skipped optional capabilities,
   unmatched-patch reasons, bounded claim summaries.
2. CLI/MCP commands or flags on existing inspection (e.g. project inspection,
   `config`) plus a plugin list/inspect view; schema'd JSON.
3. Diagnostics: use `omni_diagnostic` codes for each plugin failure class with actionable
   hints (unsupported feature, incompatible capability, binding errors, rejected pass).
4. Notices deduplicated and published only after pass success; redaction applied.

## Acceptance

- Snapshot tests per surface; redaction of paths/values; bounded output on large
  workspaces.
- Notices absent for rejected passes.
- Docs pages for plugin configuration and inspection.
