# P09: Plugin sources, manifests and captured artifacts

**Size:** L · **Depends on:** F08, F02, P02 · **Unblocks:** P13

## Goal

Let enabled `local`/`git` plugins resolve through the shared source lifecycle and become
verified descriptors, without executing code during installation.

## RFC references

- [§15 remote-source integration](../../015.00-remote-source-integration.md)
- [§17.6 artifact digests vs mutable loading](../../017.00-review-findings-and-prerequisites.md#176-artifact-digests-versus-mutable-artifact-loading)

## Work

1. `PluginRemoteContributor` (subsystem id `plugin`) in
   `crates/omni_remote_source_contributors/src/plugin.rs`; construct from loaded
   activation config only. Add plugins to `SubsystemSelection` through `omni_api`, CLI
   and MCP schemas and install reports; empty selection means all, including plugins.
2. Materialize enabled git sources via the shared manager (locked commits reused,
   explicit update only advances mutable refs, offline needs local content with an
   actionable hint). Disabled entries cause no read/fetch.
3. Validate static manifests, infer omitted ids, check enabled-id uniqueness, module
   closure and artifact-relative paths without running code.
4. Return full reference set for `refs/plugin.json`; always run when selected, even
   with zero plugins (writes the empty set). Lazy resolution keeps the complete set.
5. Hold F08 active-use leases while descriptors/modules may be read.
6. **Captured bytes:** hash and load the same bounded bytes of manifest/module closure;
   never reopen mutable paths; deny later module resolution outside the snapshot.

## Acceptance

- Concurrent acquire/update/GC; offline missing-pin errors; failed install keeps old
  references.
- Entry/import mutation, symlink swap, edit between hash and instantiation fail safe.
- Paths cannot escape the artifact directory; builtins need no fetch.
- Source installation never runs hooks.
