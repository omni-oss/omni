# R01: Extract `omni_source_configurations`

**Type:** refactor · **Size:** S · **Depends on:** none

## Goal

Move the generic source definitions (`SourceConfig`, its variants, `SourceConfigProfile`)
out of `omni_configurations` into a new low-level crate, with re-exports so existing
import paths keep working.

## Why it matters

Standalone: breaks a heavy crate's ownership of a shared concept; other subsystems
(packs, generators, tools) already use these types. For plugins: a future
`omni_plugin_configurations` must depend on source types without depending on
`omni_configurations`, or `omni_configurations` (which references plugin activation)
would form a cycle.

## RFC references

- [§13.1 new crates and dependency direction](../../013.00-implementation-plan.md#131-new-crates)
- [§18.3 source/profile row](../../018.00-readiness-and-open-decisions.md#183-implementation-gates-grounded-in-current-code)

## Code starting points

- `crates/omni_configurations/src/source.rs`
- Users: `omni_remote_source`, `omni_remote_source_contributors`, generator/tool
  configuration crates _(grep `SourceConfig`)_

## Work

1. Create `crates/omni_source_configurations` depending only on existing low-level
   config/path/URL types.
2. Move generic `SourceConfig`, variants and `SourceConfigProfile`. Subsystem-specific
   profiles and validators **stay** with their owners.
3. Re-export from `omni_configurations` at the old paths.
4. Do not add a `builtin` variant (a later plugin-only wrapper).
5. Add a dependency-direction test (e.g. `cargo tree`/`cargo deny`-style check or a unit
   test) so the new crate cannot depend on `omni_configurations`.

## Out of scope

- Any plugin types. New behavior.

## Acceptance

- Whole workspace builds with no import changes required outside the moved code.
- Serde and JSON-schema output byte-identical (snapshot/parity test).
- No cycles; new crate has minimal dependencies.
