# F02: Projection guardrails for `plugin.omni.*` control files

**Type:** feature (small) · **Size:** XS · **Depends on:** none

## Goal

Add `plugin.omni.{ext}` (all supported config formats) to the shared control-plane
manifest names that projection guardrails protect, using the existing
`allow_omni_config` override for both individual destinations and directory copies.

## Why it matters

A projection must not be able to overwrite a plugin manifest by accident. The change is
harmless without plugins and avoids a later security gap.

## RFC references

- [§9.1](../../009.00-failures-security-and-observability.md#91-projection-control-plane-protection)

## Code starting points

- `crates/omni_constants`, `crates/omni_projections` (existing control-plane name list
  and the destination/directory-copy checks) _(locate by grepping an existing protected
  name such as workspace/project config filenames)_

## Work

1. Add the manifest name across supported formats where existing names are enumerated.
2. Reuse the same override; no plugin-specific bypass.
3. Do **not** protect `package.json`, `Cargo.toml`, lockfiles or other ecosystem
   markers.

## Acceptance

- Tests mirroring existing protected-name tests: single destination and directory copy
  blocked; allowed with `allow_omni_config`.
- A projection of `package.json` is unaffected.
