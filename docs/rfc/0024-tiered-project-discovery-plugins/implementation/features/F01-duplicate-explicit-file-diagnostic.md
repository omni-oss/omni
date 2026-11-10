# F01: Reject duplicate explicit project files at one canonical root

**Type:** feature (validation) · **Size:** S · **Depends on:** none

## Problem

Today a root can have multiple selected explicit project files (for example
`project.omni.yaml` and `project.omni.yml`, or several extension variants). Their
interaction is undefined or order-dependent. Plugin inference needs at most one explicit
project file per canonical root to compose against.

## Goal

Detect two or more **selected** explicit project files at the same canonical root before
any loading/inference and fail with every conflicting path listed.

## RFC references

- [§6.2 pipeline step 4](../../006.02-discovery-pipeline-and-lockfiles.md#62-pipeline)
- [§10 required tests](../../010.00-implementation-and-rollout.md) ("Multiple selected explicit project files…")

## Code starting points

- `crates/omni_context/src/project_discovery.rs` (candidate grouping)
- `crates/omni_context/src/context.rs` (`discover_project_paths`)
- Diagnostics conventions: `crates/omni_diagnostic`, `omni_codes` _(follow RFC 0021)_

## Work

1. Group selected explicit files by canonical root (resolve symlinks consistently with
   existing discovery; treat `yml`/`yaml` aliases and format variants as the same root).
2. If a group has more than one file, fail with a structured diagnostic listing **all**
   paths. Must be independent of discovery order.
3. Do not flag: separate explicit **base** files referenced via `extends`, or multiple
   ecosystem markers that are not explicit project files.
4. Changelog: documented zero-plugin behavior change (the only intentional discovery
   selection change besides layer corrections).

## Out of scope

- Inference controls conflict handling (plugin-specific).

## Acceptance

- Tests: `yml`+`yaml`, differing formats, three files, reordered discovery.
- Ignored/excluded files are not counted.
- Extended base files at other paths remain valid.
- Diagnostic includes all paths and an actionable fix hint.
