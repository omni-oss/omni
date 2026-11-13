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
5. **Land:** merge commit (or squash) into `main`.
6. **Delete** the branch.

## 4. Rules

- Each branch has a named owner.
- Lifespan 1-2 weeks, hard cap about 4.
- Cap of roughly 15-25 child PRs.
- No force-push on integration branches.
- Hotfixes go to `main`, then are synced into active branches.
- Required check: the single `CI Passed` gate (see section 6). Which jobs run
  depends on the target: child PRs run only the fast jobs; PRs to `main`, pushes
  to `integration/**` and nightly runs also run the full jobs.
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

1. **Gate on the run:** ignore runs that are not pushes to an `integration/**`
   branch. Ignore any run whose head SHA is not the current branch head, so an
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
  cancel-in-progress) for every change to `red` or the freeze. Inside it, it
  re-reads the branch head immediately before applying each change. If the head
  moved, it discards the result and sets `red` from the newest completed run on
  the new head, so a stale run cannot leave the branch frozen or unfrozen.
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

Other workflows:

| Workflow                | Trigger                       | Purpose                                                                                |
| ----------------------- | ----------------------------- | -------------------------------------------------------------------------------------- |
| `integration-sync`      | push to `main`, daily, manual | merge `main` into each `integration/*`; on conflict abort, comment, label `needs-sync` |
| `integration-track`     | child PR merged               | append `- [x] #N title (@author)` under `<!-- children -->` in the integration PR body |
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
