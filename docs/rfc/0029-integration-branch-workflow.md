# RFC 0029: Integration Branch Workflow

- **Status:** Draft (Phase 1: merge queue; Phase 2: integration branches, deferred)
- **Owner:** TBD (name a pilot owner before rollout)
- **Date:** 2026-11-12
- **Base branch:** `main`
- **Affected areas:** `.github/workflows`, branch protection, contributor docs

## 1. Summary

Full CI is slow. The plan has two phases:

- **Phase 1 (do first): merge queue.** PRs run quick CI only after explicit
  activation following queue enforcement. GitHub's merge queue
  runs the full suite on batches before they land on `main`. See section 1.1.
- **Phase 2 (fallback): integration branches.** Sections 2-6 describe this
  design. Build it only if Phase 1 does not fix the problem (see "Exit
  criteria" in section 1.1).

Phase 2 in brief:

Batch pull requests through a short-lived `integration/*`
branch. Child PRs merge into the branch with fast CI only (the `check-*` jobs
and the TypeScript tests). The integration PR to `main` runs the full suite,
and every push to the branch also gets its own full run. The branch is deleted
after it lands.

Per-commit full CI keeps attribution simple: the culprit of a red branch is the
first red commit after the last green one. Bisect is a fallback for when runs
are missing, not the main path.

### 1.1 Phase 1: merge queue

### Design

- Add a `merge_group` trigger to `ci.yaml`. Without it, queued PRs never get a
  status.
- `plan` computes
  `full: ${{ github.event_name != 'pull_request' || vars.QUICK_CI_ENABLED != 'true' }}`.
  PRs run full CI by default. Only the repository Actions variable
  `QUICK_CI_ENABLED` set to `true` (case-insensitive) enables quick CI on
  `pull_request`; unset, `false` and other values keep full PR CI. `merge_group`,
  `push` to `main`, tag pushes and `workflow_dispatch` are always full
  (`workflow_dispatch` still needs a trigger before it can be used).
- Always run: `check-rust`, `check-typescript`, `build-typescript` and
  `test-typescript`. Gate `build-debug-rust` and `test-rust` together on
  `needs.plan.outputs.full == 'true'`, because `test-rust` reuses the `target`
  dir from the debug build and would otherwise rebuild everything.
- `CI Passed` (`ci-gatekeeper`) already treats `skipped` as passing, so it stays
  the only required check for both the PR run and the queue run.
- Concurrency: apply `cancel-in-progress` to `pull_request` runs only. Queue
  runs must not be cancelled.
- Ruleset for `main` (admin required): enable **Require merge queue**, keep
  `CI Passed` as the required check, keep the merge commit method, start with
  a maximum group size of 3-5, and leave "merge when checks fail" off.
- Only after confirming that enforcement, an admin sets `QUICK_CI_ENABLED`
  to `true`. Before disabling queue enforcement, unset it or set it to `false`
  to restore full PR CI. The pilot owner owns this activation switch; remove
  it only if full PR CI is restored unconditionally or a replacement verifies
  queue enforcement before allowing quick PR CI.
- Commit messages: queued commits are still checked by `check-pr-message.yaml`.
  Keep the existing check.

### Caveats

- Merge queue availability depends on the plan of the `omni-oss` organization.
  Verify before starting.
- A PR can pass quick CI and then fail in the queue. That PR is dropped and its
  author fixes it. `main` stays green. Slow failures appear after the author
  clicks merge. If that is painful, add a label or manual trigger that runs
  full CI on a PR early.
- Verify that the `ci.yaml` concurrency group does not cancel queue runs.
- Fix the `"!./apps/site/**"` path filter (the leading `./` probably never
  matches).

### Exit criteria

Measure before and after: PR time to green, queue time to land, and queue
ejection rate. Move to Phase 2 only if the queue does not give acceptable time
to land, or the team needs a long-lived shared branch to stabilize a group of
changes together.

## 2. Terms (Phase 2)

- **Integration branch:** `integration/<yyyy-mm-dd>[-<suffix>]`, using the
  start date. The suffix is optional, for a same-day collision or a clarifying
  topic. The date sorts chronologically and shows a branch's age against the
  lifespan cap.
- **Integration PR:** the PR from the integration branch to `main`.
- **Child PR:** a PR targeting an integration branch.

## 3. Lifecycle

1. **Open:** an owner starts a batch with the `new-batch` workflow dispatch,
   which cuts the branch from `main` and opens a draft integration PR (see
   "Rotation").
2. **Develop:** child PRs squash-merge into the branch.
3. **Sync:** merge `main` into the branch daily (merge, never rebase).
4. **Freeze and stabilize:** stop feature merges, fix what full CI finds.
5. **Land:** merge commit into `main`. The integration PR is not squashed, so
   `main` keeps one commit per child PR, each with its `(#N)` reference, plus
   the landing merge. This keeps per-PR attribution and lets one bad PR be
   found and reverted on `main` after landing. **An integration branch is never
   squashed, and there is no exception.** Squash and rebase merge are disabled
   for PRs from `integration/**` to `main`, so only a merge commit can land
   it. This preserves per-PR history and keeps child PR retargeting correct
   (see "Rotation"). Only child PRs into the branch are squashed; the
   branch's own history is never rewritten or collapsed.
6. **Delete** the branch and **rotate**: the landed branch loses `default-batch`,
   and an owner starts the next one on request (see "Rotation").

### Merge method change

`CONTRIBUTING.md` says PRs merge with merge commits. This RFC changes that for
child PRs: squash is the only allowed method for PRs into `integration/**`
(merge commit and rebase are disabled, with no exception), while PRs to `main` still
use merge commits, and the integration PR can only use a merge commit.
Enforce it with a ruleset that restricts allowed merge methods per target
branch and head, and update `CONTRIBUTING.md` in the same change.

## 4. Rules

- Each branch has a named owner.
- Lifespan 1-2 weeks, hard cap about 4.
- Cap of roughly 15-25 child PRs.
- No force-push on integration branches.
- Hotfixes go to `main`, then are synced into active branches.
- Required check: the single `CI Passed` job in `ci.yaml` (see section 6).
  Which jobs run depends on the target: child PRs run only the fast jobs; PRs
  to `main`, pushes to `integration/**` and nightly runs also run the full jobs.
- **Commit messages.** `check-pr-message.yaml` has no branch filter, so it runs
  `cog check` on every push, including pushes to `integration/**`. `cog.toml`
  has `from_latest_tag = false`, so it checks all history: one bad commit on an
  integration branch, which has no force-push, keeps the check red on that
  branch and on `main` after landing. A squash commit message is written at
  merge time, so it cannot be checked beforehand. Instead:
    - Set the squash commit default to "PR title".
    - Add a required child-PR check that runs `cog verify` on the PR title.
    - Merge commits from syncing `main` are fine (`ignore_merge_commits = true`).
- Child PRs branch from the integration branch. Exceptions branching from
  `main`: hotfixes, CI/infra changes, small unrelated work. Stacked PRs branch
  from the PR they depend on.

### Parallel branches

Default is one active branch. At most two: to cover the freeze overlap or
independent areas. Land in order, never stack them, declare scope in each PR
description, and mark one `default-batch`.

## 5. Failure attribution

Workflow `integration-attribute`, triggered by `workflow_run` (completed) of
`CI` on pushes to `integration/**`. It runs from the default branch's copy of
the workflow, with `INTEGRATION_BOT_TOKEN`. It accepts a run only if
`workflow_run.head_repository.full_name == github.repository`, its branch is
`integration/**` and its event is `push` or `workflow_dispatch` (the nightly
path, below).

### Per-commit results

Each push to `integration/**` gets its own full `CI` run (see "Concurrency" in
section 6), so every commit on the branch has a result.

### Steps

1. **Attribute every failed run.** A failed run on an older commit still
   identifies a culprit, so attribute (comment) regardless of the current head.
   Only the `red` label and the freeze are gated on the head (see "Serialize
   state changes").
2. **Collect failures:** read the per-task results that each CI job uploads
   (see "Failure artifacts"), not the GitHub job list.
3. **Flake filter by attempt.** A rerun completes `CI` again and fires this
   workflow again, so the decision is a table on `run_attempt`:

    | Attempt | Result  | Action                             |
    | ------- | ------- | ---------------------------------- |
    | 1       | failure | `gh run rerun --failed`, then exit |
    | 2       | success | record a flake, stop               |
    | 2       | failure | final: continue to step 4          |

4. **Find the culprit:** walk first-parent commits back from the failing
   commit to the last commit whose `CI` run succeeded. Squash commits are
   `(#PR)`, and a sync merge from `main` is its own entry.
    - One commit in range, or the first red commit after the last green one:
      it is the culprit.
    - A gap in results (a run is missing, for example a push `CI` ignored): use
      the bisect fallback below on the range.
5. **Map to a PR:** commit subject `(#N)` gives the child PR; a sync merge
   commit maps to the integration PR instead.
6. **Verify (bisect fallback only):** re-run the failing tasks at the suspect
   commit and at its parent. Report "confirmed" only if it fails at the commit
   and passes at the parent.

### Bisect fallback

Used only when the range has more than one commit without results.

- `git bisect start --first-parent`, then `git bisect run`, running only the
  failing omni tasks (`omni run <task> -p <project>`), not the whole suite.
  First-parent keeps bisect on the same history as the range, so it can name a
  sync merge but never a commit from `main`'s history.
- Cap the bisect at N steps and a time budget. If exceeded, report `suspected`
  with the remaining range.
- Each step needs the same toolchain as the failing job (for example Rust), so
  it reuses `determine-build-tools` and `setup-tools`.
- `-f=failed` is used in the CI commands, and in `determine-build-tools` via
  `run-omni-jobs`. It must not be reused during bisect, which needs a fresh
  result at every commit.
- Run bisect on the failing lane's runner (`matrix.runner`) with its `TARGET`
  env. The multiplatform matrix covers Windows, macOS and cross-compiled
  targets, which a single Linux runner cannot reproduce. For a lane bisect
  cannot run on, report `unknown`.

### Failure artifacts

GitHub job names cannot be mapped back to omni tasks. `run-omni-jobs.yaml`
names its jobs `Run Jobs` and `Run Multiplatform Jobs` (matrix by target), and
each job runs many tasks in one command:
`omni -i run -f=failed <tasks> -p <project> -p <project> ...`.

Omni already writes per-task results: `plan` uses `--result=result.json`, with
`task_name`, `project_name`, a `status` (completed, errored, skipped), an exit
code and the task command. So:

1. In both jobs of `run-omni-jobs.yaml`, add `--result=result.json` to
   `Run Tasks`, and upload it with `if: always()` as an artifact named
   `omni-results-<job>-<target>`.
2. In the multiplatform job, include the matrix `target` and `runner` in the
   artifact, since a failure may be target-specific.
3. `integration-attribute` downloads these, keeps entries that failed, and
   builds one `omni run <task> -p <project>` command per failed task.

### Diagnostics output

Comment on the culprit child PR and on the integration PR, and set a `red`
label on the integration PR:

- Culprit commit, PR and author.
- Failing jobs and tasks, with log links and the first error lines.
- Last green commit and the commits in range.
- Confidence: `confirmed`, `suspected` (bisect hit a limit), or `unknown`.
- Reproduce command, for example `omni run test:unit -p <project>` at
  `<sha>`.
- Suggested action: fix forward within about 24 hours, or `git revert <sha>`.

The `red` label is removed when a later run on the current head goes green.

### No automatic fixes or reverts

No agent generates fixes and the bot opens no revert PRs. The bot only
diagnoses: it points out which PR caused the failure and leaves the fix to
people. A revert is a suggestion for the owner.

### Fix process

The merged culprit PR cannot be reopened and the branch has no force-push, so
every fix is a new child PR into the integration branch.

1. **Notify:** the bot comments on the culprit PR, tags its author and the
   branch owner, and adds `red`.
2. **Acknowledge:** the author replies within about 2 working hours. Otherwise
   the branch owner takes over or reassigns.
3. **Fix forward (default):** the author cuts `fix/<sha7>-<task>` from the
   current integration branch head, reproduces with the command in the
   diagnostics, and opens a child PR that links the culprit PR (`Fixes #N`). It
   runs the normal child PR CI and needs one approval from the branch owner or
   a reviewer.
4. **Revert (fallback):** if no clear fix within about 24 hours, or the branch
   blocks others, the branch owner opens a `git revert <sha>` child PR. The
   author re-lands the change later as a new PR. This is for child squash
   commits only. If the culprit is a `main` sync merge, do not revert it:
   `git revert -m 1` would also undo the `main` changes, and the next sync
   would not bring them back. Instead fix forward on the integration branch, or
   have the owner resolve the conflict that the sync exposed in a child PR.
   The integration branch is not rewritten, and `main` is never edited to
   suit it.
5. **Confirm:** full CI runs on the branch after the fix merges. When green,
   the bot removes `red` and lifts the merge freeze. This applies only to a
   run whose SHA is the current branch head.

Roles: the author owns the fix; the branch owner unblocks (reassign, revert,
fast-track approval); the bot diagnoses, tags and labels.

### Freeze enforcement

While `red`, only fix and revert PRs may merge. This is enforced by a required
child-PR check, `integration-freeze`. It fails when the target branch's
integration PR is labelled `red` or `frozen`, unless the child PR is labelled
`fix` or `revert`. It re-runs on `labeled` and `unlabeled` events. The branch
owner may approve fix and revert PRs as the sole reviewer.

### Other rules

- Bisect cannot see failures caused by interaction between two children. In
  that case report `unknown`, list the range and ask the owner to triage.
- Bisect needs deterministic tests. Flaky or order-dependent failures reach
  `unknown` rather than a wrong culprit.
- Only the failing tasks run during bisect, on a clean checkout per step.
- **Isolation.** Bisect runs code from merged child PRs, so it is untrusted
  relative to `INTEGRATION_BOT_TOKEN`, and a job started by `workflow_run` runs
  in `main`'s cache scope. A separate cache key prefix does not help: `setup-omni`
  adds the catch-all restore key `${RUNNER_OS}-omni-`
  (`.github/actions/setup-omni/action.yml`), so a `main` run can restore any
  `<os>-omni-*` entry. So:
    - `report` does not run bisect itself. It starts a separate `integration-bisect`
      workflow with `workflow_dispatch --ref integration/<x>`, so its caches are
      scoped to that branch, which `main` cannot restore. Alternatively, bisect
      uses `actions/cache/restore` only and never saves.
    - `integration-bisect` has `contents: read` only, no bot token and no other
      secrets, and uploads its result as an artifact.
    - The privileged `report` job downloads that artifact and only parses it as
      data (culprit SHA, confidence, log excerpt). It never executes anything
      from the artifact or the checked-out commits. The rerun and labelling
      steps run in `report`.
- **Serialize state changes.** The head check is not enough, because
  attribution takes time and a newer run can finish meanwhile. The `report` job
  uses a `concurrency` group per branch (`integration-state-<branch>`, without
  cancel-in-progress) for every change to `red` or the freeze. A push cannot be
  serialized with these jobs, so the head is checked on both sides of the
  change:
    1. Re-read the branch head immediately before applying the change. If it
       moved, discard the result and reconcile (below).
    2. Apply the change.
    3. Re-read the head again. If it moved during the update, reconcile.
       Reconcile means: find the newest completed `CI` run on the new head, and
       set `red` and the freeze from that run, repeating the before/after
       check. Cap the loop at a few iterations; if the head is still moving,
       leave state unchanged and let the next completed run reconcile.
- **New head with no completed run.** Leave `red` and the freeze as they are
  until a run on that head completes. The completion of that run triggers this
  workflow and reconciles. A branch that was red stays frozen, and a branch that
  was green stays open, rather than guessing. If no run starts for the head
  (for example, a push that `CI` ignores), the next run on any later head, or
  the nightly run, reconciles.

### Nightly run

A `schedule` trigger only runs on the default branch, so it cannot run `CI` on
an integration branch directly. A small `integration-nightly` workflow, on a
`schedule`, lists open `integration/**` branches and starts `CI` on each one's
current head with `gh workflow run ci.yaml --ref <branch>`. This needs a
`workflow_dispatch` trigger on `ci.yaml`. Dispatch with `INTEGRATION_BOT_TOKEN`,
consistent with the other bot pushes; verify during the pilot that the
resulting `workflow_run` reaches `integration-attribute`.
The dispatched run has event `workflow_dispatch` and the `full` gate is true
for it, so its result is accepted and reconciles `red` and the freeze for that
head. It also catches a head that `CI` ignored on push.

## 6. Automation

Do not split CI into separate `fast` and `full` workflows. Keep `ci.yaml` and
gate each job.

### Triggers and the `full` flag

Add `integration/**` to the `pull_request.branches` and `push.branches`
triggers and compute one flag in `plan`:

```yaml
full: ${{ github.base_ref == 'main' || github.event_name != 'pull_request' }}
```

This replaces the Phase 1 flag: only child PRs targeting `integration/**`
run quick CI. PRs to `main`, including integration PRs, run full CI regardless
of `QUICK_CI_ENABLED`.

A push to an integration branch with an open integration PR triggers both a
full `push` run and a full `pull_request` run. Keep both: the push result is
used for per-commit attribution, while the PR's required `CI Passed` waits for
the full suite on the test merge commit. A quick PR run cannot safely rely on
the full push check on the head SHA: GitHub uses the test merge commit's check
when it has one ([required-check selection][required-check-selection]).
Deduplicating these runs is deferred until a gate can validate the full result
before reporting success on the PR.

[required-check-selection]: https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/troubleshooting-required-status-checks#conflicts-between-head-commit-and-test-merge-commit

### What is fast and what is full

`ci.yaml` has no separate unit and integration test jobs. Its jobs are
`check-rust`, `check-typescript`, `build-debug-rust`, `build-typescript`,
`test-rust` and `test-typescript`. `test-rust` depends on `build-debug-rust`
and reuses its `target` dir, so gating only the build job would make
`test-rust` rebuild everything and save nothing. So define the split in terms
of real jobs:

- **Fast (always run):** `check-rust`, `check-typescript`, `build-typescript`
  and `test-typescript`.
- **Full (gated by `needs.plan.outputs.full == 'true'`):** `build-debug-rust`
  and `test-rust`, together, and anything else slow.
- Optionally split a unit-only Rust task later (`test:unit` /
  `test:integration` omni tasks) to move fast Rust coverage into child PRs.

Measure the saving before rollout (see section 8). The RFC has no CI duration
data yet.

`ci-gatekeeper` is the `CI Passed` job in `ci.yaml`. It already treats
`skipped` as passing, so `CI Passed` stays the only required check on every
branch.

### Concurrency

The current group is `${{ github.workflow }}-${{ github.ref }}`. GitHub keeps
only one running and one pending run per group, so a third push replaces the
pending run even with `cancel-in-progress: false`, and some commits would get
no result. For pushes to `integration/**`, put the commit in the group:

```yaml
concurrency:
    group: >-
        ${{ github.workflow }}-${{ github.ref }}-${{
          (github.event_name != 'pull_request' && startsWith(github.ref, 'refs/heads/integration/'))
          && github.sha || '' }}
    cancel-in-progress: ${{ github.event_name == 'pull_request' }}
```

Nightly `workflow_dispatch` runs on the same head share a group with the push
run for that SHA, which is fine.

### Security of CI on integration branches

`ci.yaml` has `permissions: contents: write`, and `run-omni-jobs` uses
`secrets: inherit` plus an `ENV_*` export. Integration pushes run merged child
code with these. Scope `contents: read` for non-tag runs, and keep secrets out
of runs on `integration/**`.

Also, the `ci.yaml` path filter `"!./apps/site/**"` probably never matches
because of the leading `./`. Fix it, since it decides which pushes `CI`
ignores (see "New head with no completed run").

### Rotation

"Default" means the **default integration branch**: the one open
`integration/**` branch labelled `default-batch`. It is never the repository's
default branch. `main` stays the default branch, and no workflow changes branch
protection or repository settings.

Rotation is lazy: no branch exists between batches, and `new-batch` creates
the next one on request.

Workflow `integration-rotate` runs on `pull_request` closed, from the default
branch's copy, with `INTEGRATION_BOT_TOKEN`. It acts only when the PR was
merged, its head is `integration/**` and its base is `main`:

1. Delete the landed branch. GitHub retargets open child PRs to the merged
   PR's base (`main`) when the head branch is deleted after a merge, and
   because the landing is a merge commit, their diff against `main` is clean
   without any sync. Comment on each open child PR that it now targets `main`,
   and that it can be retargeted to a parallel integration branch if one is
   open.
2. Remove `default-batch` from the landed branch's PR. The PR-target guard and
   the `git start` helper tell contributors to branch from `main` while there is
   no default.

The integration PR is never squashed (section 3), so no rebase handling for
squash landings is needed.

An owner starts the next batch with the `new-batch` workflow dispatch. It
creates `integration/<yyyy-mm-dd>[-<suffix>]` from the current `main` head,
using the suffix rule from section 2 (for example `-2`) if that ref already
exists, and checks the parallel limit (at most two active branches), refusing
to create a third. GitHub cannot open a PR for a branch with no commits beyond
`main`, so the bot adds one empty marker commit, `chore: start integration
batch <name>` (a valid conventional commit, so `cog check` passes when it
lands). It then opens the draft integration PR with the `<!-- children -->`
marker, labels it `default-batch` and assigns the owner. Until then, `main` is
the base for new work.

Why lazy: a branch created for a quiet week sits empty, goes stale and reaches
the lifespan cap without any work in it.

Rules:

- Only one branch is labelled `default-batch` at a time. The bot moves the label
  and never leaves two.
- A closed-unmerged integration PR does not rotate. The label stays where it is
  and the owner decides.
- A revert of a landed integration PR is a normal change to `main` and does not
  rotate or reopen the branch.
- The bot acts from the default branch's workflow and runs no code from any
  PR.

### Other workflows

| Workflow                | Trigger                       | Purpose                                                                                |
| ----------------------- | ----------------------------- | -------------------------------------------------------------------------------------- |
| `new-batch`             | manual (`workflow_dispatch`)  | create the next integration branch and draft PR (see "Rotation")                       |
| `integration-sync`      | push to `main`, daily, manual | merge `main` into each `integration/*`; on conflict abort, comment, label `needs-sync` |
| `integration-track`     | child PR merged               | append `- [x] #N title (@author)` under `<!-- children -->` in the integration PR body |
| `integration-rotate`    | integration PR merged         | delete the landed branch, move `default-batch` (lazy, no new branch)                   |
| `integration-nightly`   | schedule                      | dispatch `CI` on each open `integration/**` head, as the reconcile backstop            |
| `integration-attribute` | `workflow_run` of `CI`        | failure attribution (section 5)                                                        |
| `integration-bisect`    | `workflow_dispatch`           | bisect fallback, unprivileged (section 5)                                              |
| `integration-freeze`    | child PR, label events        | required check: fail while the target is `red` or `frozen` (section 5)                 |
| PR title check          | child PR                      | required check: `cog verify` on the PR title (section 4)                               |
| PR-target guard         | PR to `main`                  | see below                                                                              |

**PR-target guard.** Fails a PR to `main` if its head contains any first-parent
commit of an open `integration/**` branch beyond `main`
(`git merge-base --is-ancestor`). Exemptions are by label (`hotfix`), and the
integration PR itself (head `integration/**`) is always allowed.

Pushes made with `GITHUB_TOKEN` do not trigger workflows. Sync and tracking use
`INTEGRATION_BOT_TOKEN` (GitHub App or PAT), which needs a branch-protection
bypass on `integration/**`.

## 7. Alternatives

- Merge queue. Chosen as Phase 1 (section 1.1): it gives batching and per-PR
  attribution natively.
- Affected-only CI.
- Sharding and caching.

These are cheaper to adopt and should be tried first if full CI duration or PR
volume is modest.

## 8. Open questions

- Actual full-CI duration and PR volume. Collect these before rollout (step 1
  below), not after.
- Whether to ship a `git start` helper and rulesets as code.

## 9. Rollout

### Phase 1: merge queue

1. Record baseline: full-CI duration, PR time to green and PR volume.
2. Verify the plan of the `omni-oss` organization supports merge queue.
3. Land this RFC. Update `CONTRIBUTING.md` (full PR CI by default, quick PR CI
   only after activation, full CI in the queue, merge via the queue).
4. In `ci.yaml`: add `merge_group`, the `full` flag, the job gating,
   the concurrency change and the path filter fix.
5. An admin enables **Require merge queue** in the `main` ruleset with
   `CI Passed` required and confirms enforcement before setting the repository
   Actions variable `QUICK_CI_ENABLED` to `true`. If queue support is unavailable,
   leave the variable unset and keep full PR CI.
6. Verify PRs run quick CI only after activation and queue runs still require
   Rust builds and tests. To roll back, unset `QUICK_CI_ENABLED` or set it to
   `false` before disabling queue enforcement.
7. Run for a few weeks and compare against the baseline (exit criteria in
   section 1.1).

### Phase 2: integration branches (only if Phase 1 falls short)

1. Add `integration/**` triggers, the per-commit concurrency group and the
   `contents: read` scoping to `ci.yaml`, and the result artifacts to
   `run-omni-jobs.yaml`, then the sync and track workflows.
2. Add the PR title check, `integration-freeze`, the attribution bot and the
   PR-target guard.
3. Pilot with one integration branch and a named owner.
