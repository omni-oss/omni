# P02: `omni_plugin_configurations` and `plugins` workspace config

**Size:** M · **Depends on:** R01 · **Unblocks:** P03, P04, P05, P09

## Goal

Data-only crate owning activation entries, manifests, capability/version/hook/claim
declarations, options and limit configuration, plus the `plugins` key in workspace
configuration (empty by default) with validation.

## RFC references

- [§5 activation and identity](../../005.00-activation-identity-and-delivery.md), [§5.1 inference dependencies and bindings](../../005.00-activation-identity-and-delivery.md#51-declared-inference-dependencies-and-consumer-bindings)
- [§13.1 crates and dependency direction](../../013.00-implementation-plan.md#131-new-crates)
- [§14.2 negotiation](../../014.00-versioning-compatibility-and-migrations.md#142-negotiation)

## Work

1. New crate `crates/omni_plugin_configurations`, depending on
   `omni_source_configurations` (R01) and `omni_capabilities` (discovery permission
   profile types). It must not depend on `omni_configurations` or the future contract.
2. Entry types: `builtin`/`local`/`git` sources (builtin wrapper lives here, not in the
   shared source enum), optional `id`, `enabled` (default true when listed), `options`,
   `apply`, `inference_bindings`.
3. Manifest types (`plugin.omni.*`): `name`, `version`, `protocol`, `runtime`, `entry`,
   `capabilities[]` (`name`, `api`, `required` default true, `requires_features`,
   `hooks`, `claims.publishes/consumes`), `inference_dependencies`, static
   `discovery.markers/inputs`. Strict unknown-field rejection.
4. Static validation (no source access): id syntax, explicit-id collisions among
   enabled entries, disabled ids reserve nothing, binding slot existence where known.
5. Wire into `omni_configurations` workspace config as `plugins: []`; schema generation.
6. If compiled without plugin features (P14), a non-empty enabled list yields the
   "unsupported feature" diagnostic. Keep this residual code tiny.

## Out of scope

- Reading manifests from sources (P09). Contract types (P03). `apply`/`defaults`
  semantics (P04): only their config shape is declared here if P04 needs it; otherwise
  add in P04.

## Acceptance

- Default config unchanged; schema snapshot shows only the new `plugins` entry.
- Dependency-direction test: no cycle with `omni_configurations`.
- Unit tests: defaults (`enabled` omitted = true; `false` stops access), id collisions
  with both locations in the diagnostic, scoped-name ids unchanged, `::` reserved.
- Manifest examples from §5 parse; unknown capability fields rejected.
