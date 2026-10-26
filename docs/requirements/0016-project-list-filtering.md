# Requirements: `project list` filtering and `print-config` selection

- **RFC:** [0016-project-list-filtering](../rfc/0016-project-list-filtering.md)
- **Status:** Draft
- **Date:** 2026-10-15

## Summary

`omni project list` currently prints every project configuration omni loads. That
includes base templates and files pulled in only because another project extends
them, and none of those can actually be run. This change makes the default list
match exactly the projects `omni run` accepts. Opt-in flags bring templates and
extends-only configs back, annotated with what they are. `project print-config`
still shows any loaded config, but it now resolves name collisions predictably
and can select a config by file path. The same options are exposed through the
MCP tools.

## Goals

- By default, `project list` (CLI and MCP) returns exactly the projects that task
  execution considers runnable.
- Users can opt in to seeing base templates, extends-only configs, or every
  loaded config.
- When hidden entries are requested, the output shows what each entry is and
  where its file lives.
- The default output format stays compatible for existing scripts.
- The listing and task execution share one definition of "runnable", so they
  can't disagree again.
- `print-config` gives a predictable result when a name matches more than one
  config, and refuses to guess between two real projects with the same name.
- `print-config` can reach any loaded config by its file path, including
  templates that share a name with something else.

## Non-goals

- Hiding anything from `print-config`; it remains a debugging tool for
  inheritance.
- Making `project list` run the full workspace validation that `omni run` does.
- Narrowing filters such as by tag, by task, by glob, or affected-only.
- Changing project discovery or config merging. `extends` resolution changes in
  one way only: targets outside the workspace root are rejected.

## Requirements

### Configuration

REQ-01 No workspace or project configuration fields are added, removed, or
changed.

### Behavior

REQ-02 A loaded config must count as runnable only when workspace discovery found
it and it is not marked `base: true`.

REQ-03 By default, `omni project list` must list exactly the runnable projects.

REQ-04 The default listing must use the same runnable rule as task execution, so
every listed name is accepted by `omni run`.

REQ-05 `--include-base` must add every config marked `base: true`, whether or not
discovery found it.

REQ-06 `--include-extended` must add every config that was loaded only because
another config extends it and that discovery did not find.

REQ-07 `--all` must list every loaded config.

REQ-08 `--all` must keep meaning "every loaded config" if new hidden categories
are introduced later.

REQ-09 Visibility flags (`--include-base`, `--include-extended`, `--all`) must
only ever add entries, never remove them.

REQ-10 An entry must appear when any of the passed visibility flags includes it.

REQ-11 A base config that discovery did not find must appear under any one of
`--include-base`, `--include-extended`, or `--all`.

REQ-12 Combining `--all` with `--include-base` or `--include-extended` must be
accepted and must give the same result as `--all` alone.

REQ-13 `project list` output must be sorted by name, then by config path.

REQ-14 Without visibility flags, the output format must be unchanged: one name
per line in text, an array of names in JSON or YAML, and a `projects` key in
TOML.

REQ-15 When any visibility flag is passed, every entry must include its name, its
config file path, whether it is `base`, and whether it was `discovered`.

REQ-16 With a visibility flag, text output must show one aligned line per entry
containing the name, tags, and path.

REQ-17 In text output, the tag must be `project` for a runnable entry; otherwise
it lists `base` (if the entry is base) and/or `extended` (if discovery did not
find it).

REQ-18 With a visibility flag, JSON and YAML output must be a list of objects with
`name`, `path`, `base`, and `discovered` fields.

REQ-19 With a visibility flag, TOML output must be a list of `projects` tables
with the same four fields.

REQ-20 The output shape must depend only on which flags were passed, never on
which entries happen to be listed.

REQ-21 Listed paths must be relative to the workspace root and use `/` as the
separator on every platform.

REQ-22 `project list` must not perform the workspace validation that `omni run`
performs, such as duplicate-name, dependency, or environment checks.

REQ-23 The MCP `project_list` tool must accept optional `include_base`,
`include_extended`, and `all` parameters that default to false.

REQ-24 These MCP parameters must mean the same as the CLI flags.

REQ-25 Calling MCP `project_list` with no parameters must keep working and return
the default runnable list.

REQ-26 The MCP `project_list` result must keep its `projects` list of names.

REQ-27 The MCP `project_list` result must always include an `entries` list with
`name`, `path`, `base`, and `discovered` for each returned entry, whatever the
parameters.

REQ-28 `project print-config` must continue to resolve any loaded config,
including base and extends-only configs.

REQ-29 When exactly one runnable project has the requested name, `print-config`
must print that project.

REQ-30 In that case, same-named base or extends-only configs must be ignored
without a warning.

REQ-31 When two or more runnable projects have the requested name, `print-config`
must fail with a `duplicate project name` error.

REQ-32 That error must list every matching path and suggest `--path`.

REQ-33 When that error occurs, `print-config` must print no config.

REQ-34 The duplicate check in `print-config` must only consider the requested
name; duplicates elsewhere in the workspace must not affect it.

REQ-35 The duplicate-name error must be reported the same way, and with the same
exit code, as an unknown project name.

REQ-36 When no runnable project has the requested name, `print-config` must print
the matching base or extends-only config whose path sorts first.

REQ-37 In that case, if more than one config matched, `print-config` must warn,
list the other matching paths, and suggest `--path`.

REQ-38 `project print-config --path <PATH>` must print the loaded config stored at
that file.

REQ-39 `print-config` must require exactly one of a project name or `--path`.

REQ-40 Passing both a name and `--path`, or neither, must be a usage error with
exit code 2.

REQ-41 On the CLI, a relative `--path` must be resolved against the current
directory.

REQ-42 On the CLI, an absolute `--path` must be accepted.

REQ-43 `--path` must match discovered and extends-only configs alike, after `.`
and `..` segments are resolved.

REQ-44 A `--path` pointing at a file that is not one of the loaded configs must
be an error, even if the file exists.

REQ-45 The MCP `project_config` tool must accept an optional `path` parameter
alongside `name`.

REQ-46 MCP `project_config` must reject a call that has both `name` and `path`, or
neither, as invalid parameters.

REQ-47 MCP `project_config` must resolve a relative `path` against the workspace
root.

REQ-48 Existing MCP `project_config` calls that pass only `name` must keep
working.

REQ-49 A project config that discovery found and that another discovered project
extends must be loaded exactly once, regardless of the order of their paths.

REQ-50 An `extends` target that resolves to a path outside the workspace root
must be rejected with an error naming both the extending file and the target.

### Breaking changes

REQ-51 Previously, `project list` listed every loaded config, including base
templates and extends-only configs. After this change, it lists only runnable
projects unless a visibility flag is passed. Affects: scripts and users that
relied on seeing template names in the list.

REQ-52 Previously, MCP `project_list` returned the names of every loaded config.
After this change, `projects` contains only runnable projects unless parameters
ask for more. Affects: MCP clients and agents that expected template names.

REQ-53 Previously, `project list` printed entries in internal load order. After
this change, entries are sorted by name, then path. Affects: anyone depending on
the old order, which was never guaranteed.

REQ-54 Previously, when a name matched more than one config, `print-config`
printed whichever came first in load order. That could be a template instead of
the real project, or one of two duplicate projects. After this change, the
runnable project wins, and two runnable projects with the same name are an error.
Affects: workspaces where names collide.

REQ-55 Previously, an `extends` target outside the workspace root was loaded and
merged like any other config. After this change, it is an error. Affects:
workspaces whose projects extend configuration files outside the workspace.

## Migration / upgrade path

Most users need to do nothing.

- **Scripts that need template names from `project list`:** add
  `--include-base`, `--include-extended`, or `--all`. Note that any of these
  flags also switches the output to the annotated format, so parsers must handle
  the new shape.
- **Workspaces with two runnable projects sharing a name:** these already fail
  `omni run`. Rename one of them; to inspect each one in the meantime, use
  `print-config --path`.
- **Templates hidden behind a same-named project:** `print-config` now shows the
  project. To print the template, use `print-config --path`.
- **MCP clients:** need no changes to keep working; they can opt in to the new
  parameters and the `entries` field.
- **The loading fix (REQ-49):** needs no action. Workspaces that previously
  failed with "Node already exists" now load.
- **Projects extending files outside the workspace (REQ-50):** move the shared
  config into the workspace (or vendor a copy) and point `extends` at it.

## Technical Details

- The runnable rule and the project-loading sequence are shared between the
  listing and task execution, so the two can't drift apart. A test pins that the
  default list equals the set of projects task execution loads.
- `project list` deliberately avoids full workspace validation. That validation
  loads environment files and extracts every project, so a broken env file would
  break the listing at exactly the moment a user needs it for diagnosis.
- The `print-config` duplicate check works on the already-loaded configs and
  needs no extra loading.
- `print-config` currently reports errors (including unknown names) by logging
  them and exiting 0. The new duplicate error follows the same path; changing
  that exit-code behavior is not part of this change.
- `--path` matching is lexical, the same way `extends` targets are resolved. A
  path that reaches a config through a symlinked directory won't match and
  reports "no loaded project configuration"; it fails loudly rather than picking
  the wrong file.
- The CLI resolves relative paths from the current directory (shell convention,
  tab completion). MCP resolves them from the workspace root, because it has no
  shell directory and `project_list` entries are workspace-relative. The two
  coincide when the CLI is run from the workspace root.
- `print-config` uses an explicit `--path` flag instead of guessing from the
  argument's shape, because project names may contain `/` (for example
  `@scope/pkg`).
- The MCP result schema is fixed and additive (`projects` plus `entries`), rather
  than changing shape with parameters as the CLI does, because MCP clients rely
  on a stable output schema.
- Nothing here feeds content hashing or caching, so there is no stale-cache risk.
- The loading fix (REQ-49) is already implemented. The loader now skips a queued
  config whose file has already been loaded. It is covered by a unit test and an
  end-to-end test.
- The command reference pages for `project list` and `project print-config` on
  the docs site are generated from the CLI definition and must be regenerated.

## Out of scope

- Narrowing filters for `project list` (by tag, task, glob, or affected status).
  The flag model leaves room for them as a separate, later change.
- Flags that remove entries from the visible set (`--exclude-*`).
- Running full workspace validation in `project list`.
- Hiding base or extends-only configs from `print-config`.
- Showing each project's `extends` chain.
- Matching `--path` through symlinks via canonical paths.
- Changing `print-config`'s existing exit code for errors.
- Changes to discovery or merge semantics (`extends` changes only by rejecting
  targets outside the workspace root).
