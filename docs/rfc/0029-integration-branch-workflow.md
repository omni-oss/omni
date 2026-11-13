# RFC 0029: Integration Branch Workflow

- **Status:** Draft
- **Owner:** TBD
- **Date:** 2026-11-12
- **Base branch:** `main`
- **Affected areas:** `.github/workflows`, branch protection, contributor docs

## 1. Summary

Full CI is slow. Batch pull requests through a short-lived `integration/*`
branch. Child PRs merge into the branch with fast CI only (format, lint,
typecheck, unit). The integration PR to `main` runs the full suite. The branch
is deleted after it lands.

## 2. Terms

- **Integration branch:** `integration/<yyyy-mm-dd>[-<suffix>]`, using the
  start date. The suffix is optional, for a same-day collision or a clarifying
  topic. The date sorts chronologically and shows a branch's age against the
  lifespan cap.
- **Integration PR:** the PR from the integration branch to `main`.
- **Child PR:** a PR targeting an integration branch.

## 3. Lifecycle

1. **Open:** cut the branch from `main`, open a draft integration PR.
2. **Develop:** child PRs squash-merge into the branch.
3. **Sync:** merge `main` into the branch daily (merge, never rebase).
4. **Freeze and stabilize:** stop feature merges, fix what full CI finds.
5. **Land:** merge commit into `main`. The integration PR is not squashed, so
   `main` keeps one commit per child PR, each with its `(#N)` reference, plus
   the landing merge. This keeps per-PR attribution and lets one bad PR be
   found and reverted on `main` after landing. It also matches the repo's merge
   commit convention. Squash is an exception, only with the branch owner's
   agreement, for a small batch whose per-PR history is not worth keeping.
6. **Delete** the branch and **rotate**: the landed branch loses `default-batch`,
   and an owner starts the next one on request (see "Rotation").

## 4. Rules

- Each branch has a named owner.
- Lifespan 1-2 weeks, hard cap about 4.
- Cap of roughly 15-25 child PRs.
- No force-push on integration branches.
- Hotfixes go to `main`, then are synced into active branches.
- Required check: the single `CI Passed` gate (see section 6). Which jobs run
  depends on the target: child PRs run only the fast jobs; PRs to `main`, pushes
  to `integration/**` and nightly runs also run the full jobs.
- Every commit that lands on an integration branch must pass `cog check`,
  because `check-pr-message.yaml` runs it on `main` pushes once the batch
  lands. Check the child PR's squash commit message at the child PR, not at
  landing.
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
the workflow, with `INTEGRATION_BOT_TOKEN`.

### Steps

1. **Gate on the run:** accept only runs of `CI` on an `integration/**` branch
   whose event is `push` or `workflow_dispatch` (the nightly path, below). Ignore any run whose head SHA is not the current branch head, so an
   older run finishing late cannot change state. For a current-head run:
   success removes `red` and lifts the freeze (see "Fix process"), and only a
   failure continues to step 2.
2. **Collect failures:** read the per-task results that each CI job uploads
   (see "Failure artifacts" below), not the GitHub job list.
3. **Filter flakes:** re-run failed jobs once (`gh run rerun --failed`). If they
   pass, record a flake and stop. A rerun completes `CI` again and fires this
   workflow again, so guard it: only rerun when `run_attempt` is 1, and treat a
   failed attempt 2 as final, so a persistent failure reruns at most once.
4. **Find the range:** walk first-parent commits back to the last commit whose
   `CI` run succeeded. Squash commits are `(#PR)`, and a sync merge from `main`
   is its own entry.
5. **Narrow:**
    - One commit in range: it is the culprit.
    - Several: `git bisect start --first-parent`, then `git bisect run`,
      running only the failing omni tasks (`omni run <task> -p <project>`),
      not the whole suite. First-parent keeps bisect on the same history as the
      range, so it can name a sync merge but never a commit from `main`'s
      history. Cap the bisect at N steps and a time budget.
6. **Map to a PR:** commit subject `(#N)` gives the child PR; a sync merge
   commit maps to the integration PR instead.
7. **Verify:** re-run the failing tasks at the suspect commit and at its
   parent. Report "confirmed" only if it fails at the commit and passes at the
   parent.

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
4. Bisect runs that command with the same `TARGET` env as the failing lane.

Costs and limits:

- Each bisect step needs the same toolchain as the failing job (for example
  Rust), so it reuses `determine-build-tools` and `setup-tools`. Rust steps are
  slow; the omni cache and the run's carried `target` dir keep this bounded.
- The step cap and time budget apply. If exceeded, report `suspected` with the
  remaining range.
- `-f=failed` in the CI commands must not be reused during bisect, which needs
  a fresh result at every commit.

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

The `red` label is removed when a later run goes green.

### No automatic fixes or reverts

No agent generates fixes and the bot opens no revert PRs. The bot only
diagnoses: it points out which PR caused the failure and leaves the fix to
people.

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

While `red`, only fix and revert PRs may merge. The branch owner may approve
them as the sole reviewer.

### Other rules

- Full CI on `integration/**` pushes does not cancel in progress, so each
  commit has its own result.
- While `red`, freeze non-fix merges.
- Bisect cannot see failures caused by interaction between two children. In
  that case report `unknown`, list the range and ask the owner to triage.
- Bisect needs deterministic tests. Flaky or order-dependent failures reach
  `unknown` rather than a wrong culprit.
- Only the failing tasks run during bisect, on a clean checkout per step, with
  the omni cache enabled so unchanged tasks are cheap.
- **Isolation.** Bisect runs code from merged child PRs, so it is untrusted
  relative to `INTEGRATION_BOT_TOKEN`. Split the workflow into two jobs:
    - A `bisect` job with `contents: read` only and no bot token or other secrets.
      It uses its own cache key namespace, separate from privileged workflows and
      from `main`, and never writes to a cache they restore.
    - A privileged `report` job that downloads the `bisect` job's output as an
      artifact and only parses it as data (culprit SHA, confidence, log excerpt).
      It never executes anything from the artifact or the checked-out commits.
      The rerun and labelling steps run in `report`, not in `bisect`.
- **Serialize state changes.** The head check in step 1 is not enough, because
  attribution takes time and a newer run can finish meanwhile. The `report` job
  uses a `concurrency` group per branch (`integration-state-<branch>`, without
  cancel-in-progress) for every change to `red` or the freeze. A push cannot be
  serialized with these jobs, so the head is checked on both sides of the
  change:
    1. Re-read the branch head immediately before applying the change. If it
       moved, discard the result and reconcile (below).
    2. Apply the change.
    3. Re-read the head again. If it moved during the update, reconcile.
       Reconcile means: find the newest completed `CI` run on the new head, and set
       `red` and the freeze from that run, repeating the before/after check. Cap the
       loop at a few iterations; if the head is still moving, leave state unchanged
       and let the next completed run reconcile.
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
for it, so its result is accepted by step 1 and reconciles `red` and the freeze
for that head. It also catches a head that `CI` ignored on push.

- Do not auto-revert. A revert is a suggestion for the owner.

## 6. Automation

Do not split CI into separate `fast` and `full` workflows. Keep `ci.yaml` and
gate each job. Add `integration/**` to the `pull_request.branches` and
`push.branches` triggers and compute one flag in `plan`:

```yaml
full: ${{ github.base_ref == 'main' || github.event_name != 'pull_request' }}
```

- Always run: `check-*` (format, lint, typecheck) and unit tests.
- Gate with `needs.plan.outputs.full == 'true'`: integration tests, `build-debug-rust`
  and anything else slow.
- `ci-gatekeeper` already treats `skipped` as passing, so `CI Passed` stays the
  only required check on every branch.
- Concurrency: keep `cancel-in-progress` for PRs, but disable it for pushes to
  `integration/**` so each commit keeps a full-CI baseline for attribution.

### Rotation

"Default" means the **default integration branch**: the one open
`integration/**` branch labelled `default-batch`. It is never the repository's
default branch. `main` stays the default branch, and no workflow changes branch
protection or repository settings.

Workflow `integration-rotate` runs on `pull_request` closed, from the default
branch's copy, with `INTEGRATION_BOT_TOKEN`. It acts only when the PR was
merged, its head is `integration/**` and its base is `main`:

1. Delete the landed branch, unless it is still needed by an open child PR.
   Open child PRs of the landed branch need a new base. If a parallel
   integration branch is open, first merge `main` into it, so the landing
   merge is in the new base and a retargeted PR does not show the landed batch
   in its diff. Retarget the child PRs only if that sync succeeds and the
   landing was a merge commit (the landing commit has two parents). A squash
   landing, the owner-approved exception, does not keep the integration
   branch's commits in `main`'s ancestry, so a child PR would still show the
   landed batch. For a squash landing, never retarget automatically: comment
   on each child PR with `git rebase --onto <new-base> <landed-branch-tip>
   <child-branch>`, and retarget only after the author has rebased and the PR
   diff contains just the child's own changes. If the sync fails or no
   parallel branch is open, do not retarget either: comment on each child PR
   that it must be rebased onto `main` and reopened, or wait for the owner to
   sort out the sync (`needs-sync`).
2. Check the parallel limit (at most two active branches). Skip creation if the
   limit is reached, and comment on the landed PR.
3. Do **not** create the next branch yet (lazy creation, default). Remove
   `default-batch` from the landed branch's PR. The PR-target guard and the
   `git start` helper tell contributors to branch from `main` while there is
   no default.
4. An owner starts the next batch with the `new-batch` workflow dispatch. It
   creates `integration/<yyyy-mm-dd>[-<suffix>]` from the current `main` head,
   using the suffix rule from section 2 (for example `-2`) if that ref already
   exists. A new branch has no commits beyond `main`, and GitHub cannot open a
   PR for it, so the bot adds one empty marker commit, `chore: start
   integration batch <name>` (a valid conventional commit, so `cog check`
   passes when it lands). It then opens the draft integration PR with the
   `<!-- children -->` marker, labels it `default-batch` and assigns the
   owner. Until then, `main` is the base for
   new work.

Why lazy: a branch created for a quiet week sits empty, goes stale and reaches
the lifespan cap without any work in it. Eager creation is an option: step 3
creates the branch immediately, and the nightly run deletes any default branch
that has had no child PR merged for 7 days. Eager creation uses the same marker
commit as step 4, since the PR cannot be opened without it.

Rules:

- Only one branch is labelled `default-batch` at a time. The bot moves the label
  and never leaves two.
- A closed-unmerged integration PR does not rotate. The label stays where it is
  and the owner decides.
- A revert of a landed integration PR is a normal change to `main` and does not
  rotate or reopen the branch.
- The bot acts from the default branch's workflow and runs no code from any
  PR.

Other workflows:

| Workflow                | Trigger                       | Purpose                                                                                |
| ----------------------- | ----------------------------- | -------------------------------------------------------------------------------------- |
| `integration-sync`      | push to `main`, daily, manual | merge `main` into each `integration/*`; on conflict abort, comment, label `needs-sync` |
| `integration-track`     | child PR merged               | append `- [x] #N title (@author)` under `<!-- children -->` in the integration PR body |
| `integration-rotate`    | integration PR merged         | delete the landed branch; create the next default branch (see "Rotation")              |
| `integration-nightly`   | schedule                      | dispatch `CI` on each open `integration/**` head, as the reconcile backstop            |
| `integration-attribute` | `workflow_run` of `CI`        | failure attribution (section 5)                                                        |
| PR-target guard         | PR to `main`                  | fail if based on an integration branch, unless hotfix or the integration PR itself     |

Pushes made with `GITHUB_TOKEN` do not trigger workflows. Sync and tracking use
`INTEGRATION_BOT_TOKEN` (GitHub App or PAT), which needs a branch-protection
bypass on `integration/**`.

## 7. Alternatives

- Merge queue.
- Affected-only CI.
- Sharding and caching.

These are cheaper to adopt and should be tried first if full CI duration or PR
volume is modest.

## 8. Open questions

- Actual full-CI duration and PR volume.
- Whether to ship a `git start` helper, `frozen`/`red` label gates, and
  rulesets as code.

## 9. Rollout

1. Land this RFC.
2. Add `integration/**` triggers and the `full` gate to `ci.yaml`, and the
   result artifacts to `run-omni-jobs.yaml`, then the sync and track workflows.
3. Add attribution bot and PR-target guard.
4. Pilot with one integration branch.
