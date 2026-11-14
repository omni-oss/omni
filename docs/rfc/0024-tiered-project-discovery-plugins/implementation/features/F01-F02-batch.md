# F01/F02 implementation batch

## Scope and risk

Continue the approved RFC with F01 and F02 together: reject ambiguous selected
explicit project files, and protect plugin manifests during projections.
Small implementation batch; High risk because discovery validation and a
control-plane safety check change. The request to continue implementation
authorizes these existing work packages.

## Approach

1. After discovery selection and cache exclusions, group concrete files by
   canonical parent directory before loading any layers. Report every conflicting
   path in deterministic structured diagnostics. Do not validate the extension
   closure as additional selected files or change walker behavior.
2. Add `plugin.omni.{ext}` to the shared protected manifest templates. Keep
   `allow_omni_config` as the only existing override and leave ecosystem markers
   unprotected.
3. Exercise aliases/formats, ordering, canonical roots, ignores/excludes, cache
   exclusions, extension-only bases, and projection destinations/subtrees.
   Run tests, clippy and format checks for the affected crates.
4. Add a user-facing migration note and open one PR targeting `main`.

## Undo

Revert the batch; there is no persistent format or data migration. Existing
projection overrides continue working. No plugin runtime or discovery marker
support is introduced.

## Progress

- Implementation and regression tests complete: 206 tests pass across
  `omni_constants`, `omni_context` and `omni_projections`.
- Rust and document formatting checks pass; independent read-only review found
  no issues. The documented Clippy command passes for all three affected crates.
- An additional Clippy `--all-targets` run is blocked by existing
  `result_large_err` and `err_expect` findings in the unchanged
  `omni_context/src/diagnostic_tests.rs` (lines 180 and 245). These are outside
  this batch.
- The task graph validates (97 tasks, 16 waves). `schedule.py --check` reports
  the existing formatter-only drift: regenerating and running oxfmt reproduces
  the checked-in schedule. No dependency or wave changes are needed here.
- Full-workspace and cross-platform checks are left to CI.
- PR #101 review correction: aliases of the same filename under one canonical
  parent count once. Regression reproduced before the fix; lexical-parent and
  symlink-parent aliases now pass, while distinct filenames still conflict and
  retain all selected path spellings in deterministic diagnostics.
- Remaining F/P packages are not part of this batch.
