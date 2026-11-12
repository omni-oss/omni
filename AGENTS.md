# Agent instructions

Follow the guidelines in `docs/guidelines`. They are projected from swe-docs and not tracked, so in a fresh checkout run `bun install && omni remote-sources install && omni projection sync` first.

## Artifacts

- Design and plans: `docs/rfc/`.
- Requirements: `docs/requirements/`.
- User docs (tutorials, guides, reference, release notes): `apps/site` (content in `apps/site/content`). Keep them apart from `docs/guidelines`.

## Contributor docs

- Checks, git workflow: `CONTRIBUTING.md`.
- Tooling: `docs/tooling.md`.
- Patterns: `docs/patterns/README.md`.

## Agent rules

- Commit, push and open PRs only when asked. Never change branch protections, remotes or git config.
- Never edit generated files. Run checks with the non-interactive commands in `CONTRIBUTING.md`.
- Update the contributor docs in the same change when you alter the check commands, tooling, a pattern or the git workflow they describe.

## Risk overrides

These areas are always High risk (`docs/guidelines/core/precedence.md`):

- Caching (`crates/omni_cache`, remote cache crates): rule: Low/Medium risk shortcuts; action: changes to cache keys, storage or invalidation; reason: wrong hits or stale output silently corrupt builds; effective rule: treat as High.
- Directory walking (`crates/dir_walker`, ignore and glob crates): reason: misses or extra files change hashes and what gets processed; effective rule: High.
- Capabilities (`crates/omni_capabilities`, `omni_capability_enforcement`, `omni_capability_sys`): reason: security boundary for what tasks may do; effective rule: High.

## Agent choices

If `.agents/agent-choices.local.md` exists, read it before delegating work and use the agents it names.
