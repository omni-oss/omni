# R02: Shared `WorkspacePath` and `ProjectRootId` types

**Type:** refactor (new shared primitive) · **Size:** S · **Depends on:** none
**Unblocks:** F10, R07 (identity keys)

## Goal

Add one validated, platform-independent path identity type and one root-identity wrapper
that every later discovery feature uses for project roots, provenance, cache records and
tracked inputs. Pure types: no filesystem access, no behavior change to existing code.

## Why it matters

Standalone: gives omni a single normalized, case-preserving, `/`-separated workspace
path identity usable by inspection and cache code. For plugins: determinism across
Windows/Linux/macOS and cache-record stability depend on it.

## RFC references

- [§6.4 "Path normalization" and type table](../../006.04-hook-and-claim-schemas.md#path-normalization)
- [§13.3 phase 1 path bullet](../../013.00-implementation-plan.md#133-phases)

## Code starting points

- `crates/omni_path_utils`, `crates/omni_types` (choose the owner; low-level, no heavy
  dependencies) _(decide and record in the PR)_

## Work

1. `WorkspacePath`: UTF-8, `/` only, relative to workspace root, root is exactly `.`;
   no empty/`.`/`..` segments, no trailing `/`, no NUL, no `\` in any segment; casing
   from the directory walk, compared byte-for-byte; bounded length (configurable).
2. Two constructors: from an OS path (converts separators, strips drive/UNC/verbatim
   prefixes after proving containment) and a typed **parser** for already-normalized
   values that **rejects rather than repairs**. Idempotent.
3. `ProjectRootId(WorkspacePath)` wrapper. Eligibility is not part of the type.
4. Keep `DeclaredRelativePath` and glob patterns as separate types (do not conflate).
5. Containment/symlink checks stay in access-time code, not parsing.
6. Serde + schema support; no change to existing task-cache formats.

## Out of scope

- Replacing existing path usage. New consumers adopt it as they land.

## Acceptance

- Fixture table: Windows separators, drive/UNC, backslash-in-name (rejected), case
  variants (distinct), symlink inputs (no resolution in parser), `a//b`, `./a`, `a/`,
  `..`, root `.`, idempotence, non-normalized wire input rejected.
- Tests run on Linux with Windows-path string inputs for the OS-boundary converter
  (use platform-gated tests where real Windows paths are needed).
