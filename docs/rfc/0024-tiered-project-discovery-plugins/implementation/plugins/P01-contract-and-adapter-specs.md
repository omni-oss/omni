# P01: Close the specification gaps

**Type:** specification (documents + executable fixture inputs) · **Size:** L · **Depends on:** none
**Unblocks:** P03, P06, P10, P11, P13, P15

The RFC says the external contract is **not** implementation-ready as a frozen SDK. This
package produces the missing artifacts. It is mostly writing, so it can run now, beside
the groundwork code. Output goes in new docs/fixtures, not Rust.

## RFC references

- [§18.3 gates](../../018.00-readiness-and-open-decisions.md#183-implementation-gates-grounded-in-current-code)
- [§6.4 hook/claim/query schemas and "Remaining schema work"](../../006.04-hook-and-claim-schemas.md#remaining-schema-work)
- [§4.3 adapter specification prerequisites](../../004.01-native-ecosystem-adapters.md)
- [§7.5 fixtures C01-C26](../../007.04-composition-golden-fixtures.md)
- [§8.4 budgets](../../008.03-tier-specific-enforcement.md)

## Deliverables

1. **Schema completion.** Finish "Remaining schema work" in §6.4: full field tables,
   limits, error taxonomy, per-root disposition, resolution-only declaration spelling,
   producer/view negotiation, claim ownership/duplicates, bounded paging. Output: a
   normative `contract/` doc plus machine-readable examples.
2. **Executable fixtures.** Turn H01-H24 (§6.4) into data files (inputs + expected
   normalized outputs/errors) with a tiny validator harness. They are the contract
   tests for P03, P06, P10, P11 and P13. C01-C26 stay with their groundwork packages.
3. **JavaScript adapter spec:** supported `package.json` fields, npm/Yarn/pnpm/Bun
   manifest and lockfile formats/versions, workspace ownership evidence and nested-
   workspace boundaries, manager precedence fixtures, dependency kind mapping, local
   vs external resolution evidence, script discovery rules, safe argv, task input/
   output policy, option schema, unsupported-form behavior.
4. **Cargo adapter spec:** workspace/member/default-members rules, dependency kind
   policy (normal/build edges, dev-only omitted but retained as claims), target-specific
   and optional handling, task argv (`build`/`check`/`test` with `--manifest-path`
   and `--package`), option schema.
5. **Budget/counter design:** counter ownership and lifetimes, per-root logical
   limits, transport batching, split-attempt accounting, pass retries. Candidate values
   only; calibration happens in P15.
6. **Path/permission profile:** discovery read-only capability profile and how
   containment-at-access maps to F10.

## Acceptance

- Every H-case and adapter fixture has input, expected output and a one-line rationale.
- A reviewer from outside the author's team can implement a toy native handler from the
  docs alone (spot-check by writing one in a scratch branch).
- No open decision remains in §18.1 (it states none today; keep it that way).
- Update the RFC text where the spec resolves "remaining" notes; keep links valid.

## Notes for workers

This is where hidden design choices surface. If a choice is genuinely new, stop and ask
the RFC owner rather than deciding in a fixture.
