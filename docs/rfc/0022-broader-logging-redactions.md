# RFC 0022: Broader redaction of sensitive values in logs and diagnostics

- **Status:** Accepted <!-- Draft | Accepted | Implemented | Superseded -->
- **Owner:** TBD
- **Date:** 2026-11-03
- **Base branch:** `main`
- **Affected crates/files:** new `omni_redact` crate (the `Redactor`; depends only on `omni_constants`); `omni_constants` (new `REDACTED_PLACEHOLDER`); `omni_glob` (new `name` submodule: `NameGlobSet`); `omni_command_config` (`display.rs`, `resolve.rs`; `redact_values` becomes a wrapper); `omni_task_executor` (`batch_executor.rs`, dry-run display); `omni_configurations` (`task_configuration.rs`, `project_configuration.rs`, `workspace_configuration.rs`: new `redact` block) and the generated JSON schemas; `omni_task_context` (carry the resolved policy into the context); `omni_input_schema` (`secret` inputs feed the redactor); `omni_api` / `omni_mcp_core` (tool and generator runs); `cli/omni` and `omni_cli_core` (`-a/--arg` handling, tracing/log setup); `omni_diagnostic` (RFC 0021 renderers, once landed); `tests/omni-tests` (dry-run e2e)
- **Follows:** PR #40 (dry-run command display); issue omni-oss/omni#41; RFC 0021 (structured diagnostics)
- **Amended by:** RFC 0026 (redactors are owned by an operation, not by a subscriber)

## 1. Summary

PR #40 made the dry-run diagnostic print the fully rendered command
(`Executing task '<id>': <command>`). It redacts environment values but not
values supplied through `-a/--arg` and referenced by templates such as
`{{ args.* }}`. Issue #41 records this as a new disclosure channel and lists
three options: redact all arguments, document the current behavior, or redact
only arguments marked sensitive.

This RFC resolves #41 and generalizes the fix. It proposes a single shared
**redactor** (`omni_redact`) that holds a set of sensitive values for a run,
plus a declarative `redact` block that sets opt-in or opt-out redaction
policies for args and env at workspace, project and task level. The same
redactor also enforces the existing `secret: true` marker on tool and
generator inputs. Every output surface that can echo a rendered
value (dry-run display, log/tracing output, diagnostics and MCP payloads)
passes text through that redactor instead of each site doing its own ad hoc
replacement.

## 2. Motivation / Problem

### 2.1 Current state

- `redact_values` (`omni_command_config/src/display.rs`) replaces a list of
  values with a placeholder, longest first, on the unsplit rendered string.
- Its only caller is `resolve_task_command_for_display`
  (`omni_task_executor/src/batch_executor.rs`), which passes **all
  environment values** from `task_ctx.env_vars` and nothing else.
- Argument values reach the template context (`args.*`) and are rendered into
  the command, but are never added to the redaction set.
- Other output (`log::debug!` calls in `batch_executor.rs`, tracing spans,
  error messages that embed a rendered command, MCP responses) has no
  redaction at all.
- Tool and generator inputs already have `secret: true`
  (`omni_input_schema/src/base.rs`), documented as "must not be logged,
  echoed, cached, or persisted". It is enforced for persistence (`secret` and
  `remember` conflict) and prompting, but values are not redacted from logs
  or error output.

### 2.2 Problems

1. **Argument secrets leak in dry-run.** A secret passed as `-a token=...` and
   used in a command template is printed to the terminal and to any captured
   CI log.
2. **Redaction is a one-off.** Each new surface needs its own plumbing and
   will be forgotten, as happened with args.
3. **Over-redaction is also a defect.** Redacting every env value (current
   behavior) hides non-secret values such as `NODE_ENV=production`, which
   makes the dry-run plan less useful and can mangle unrelated output when a
   short value (`1`, `true`) matches text elsewhere in the command.

## 3. Goals / Non-goals

**Goals**

- Resolve #41 with an explicit, documented policy.
- One redactor type (`omni_redact`), built once per run, used by every output
  surface.
- A declarative `redact` block that controls redaction of task `args`, tool
  and generator `inputs`, and `env`, with both opt-in and opt-out policies.
- Cover every omni subsystem that echoes user-supplied values, not only the
  task runner (4.6).
- Keep today's safe defaults, and keep the dry-run plan informative where the
  user says it is safe.

**Non-goals**

- Not a secret manager and no secret storage or encryption.
- Not a guarantee against leaks through the executed process's own output.
  Redacting child process stdout/stderr is a possible later extension
  (section 8).
- No pattern-based secret scanning (entropy, regexes for known token formats)
  and no name-based heuristics. What is redacted is always declared.
- No change to what is executed or cached, only to what is displayed.
  Cache hashing already sees real values and is unaffected.

## 4. Design

### 4.1 Decision on #41

Of the three options in #41, this RFC picks **option 3 (redact what is
declared sensitive)**, but makes the declaration a policy rather than a plain
list, so a project can choose either direction:

- **Opt-in** (`all: false, only: [..]`): today's behavior for args.
- **Opt-out** (`all: true, except: [..]`): today's behavior for env.

Defaults preserve current behavior exactly: env is opt-out (everything
redacted), args are opt-in (nothing redacted until listed). Option 1 of #41
(redact all args) is one line of config or `--redact all` (4.4).

Name-based heuristics (`*token*`, `*key*`, ...) are dropped. They were only
needed to make a looser default safe; with explicit policies and unchanged
defaults they add surprise (false positives on `KEYBOARD_LAYOUT`, misses on
`STRIPE`) without adding safety.

### 4.2 The `redact` block

The block is named `redact` because it only controls what is displayed:
execution and cache keys are unaffected. That keeps it clearly distinct from
the existing `secret: true` input marker (4.6), which also forbids caching
and persistence, and matches the `--redact*` CLI flags.

#### Shape

```yaml
redact:
  args: [token, db.password]   # tasks: free-form `args`
  inputs: [api_key, password]  # tools and generators: schema-declared inputs
  env:
    all: true                  # opt-out: redact everything...
    except: [NODE_ENV, CI]     # ...except these
```

The three keys mirror the config keys and template contexts they govern:

| Key | Governs | Vocabulary |
| --- | --- | --- |
| `args` | A task's free-form `args` map, including `-a/--arg` values | `args:`, `{{ args.* }}`, `-a/--arg` |
| `inputs` | Schema-declared inputs of tools and generators, however the value arrives (prompt, `-v/--value`, JSON, MCP) | `inputs:`, `{{ inputs.* }}`, `-v/--value` |
| `env` | Environment values | `env:`, `{{ env.* }}` |

The lists do not apply to each other. A name that must be hidden everywhere
is listed under each key. Each key means exactly what it says, at the cost of
repeating names.

Each category (`args`, `inputs`, `env`) is a **policy**, written in one of
two forms:

| Form | Meaning |
| --- | --- |
| `[a, b]` | Shorthand for `{ all: false, only: [a, b] }` |
| `{ all: false, only: [..] }` | Opt-in: show everything except names in `only` |
| `{ all: true, except: [..] }` | Opt-out: redact everything except names in `except` |

| Field | Type | Meaning |
| --- | --- | --- |
| `all` | bool | Whether names not listed are redacted. Inherited when unset |
| `only` | list of names/globs | Always redacted. Valid with either value of `all` |
| `except` | list of names/globs | Shown, unless also matched by `only` anywhere in the chain. Only meaningful with `all: true` |

The shorthand only ever adds names to `only`; it does not set `all`. Opting
out is always spelled `all: true`, so a bare list can never silently change
which unlisted values are shown. At the root, an unset `all` takes the
category default: `true` for env, `false` for args and inputs (today's
behavior).

Under `all: true`, a value of an unlisted name is redacted only if it is at
least 4 characters long (the `bulk_min_len` rule in 4.3); a name matched by
`only` is always redacted, whatever its length. Names matched by `except` are
never redacted. This keeps blanket env redaction from hiding values like `1`
or `true` and corrupting unrelated text, while anything the user named
explicitly is hidden unconditionally.

#### Names and globs

- Entries are names, with globs allowed (`AWS_*`, `*_TOKEN`), matched by the
  name glob matcher described in 4.7. There is no `!` negation: exclusion is
  `except`, consistent with RFC 0011.
- Separators: args and inputs use `.`, so a dotted name is matched per
  segment (`*` stays within a segment, `**` crosses segments). Env names use
  no separator, so `*` matches the whole name.
- Names are case-sensitive, except env names on Windows, which are
  case-insensitive to match how the OS treats them.
- An invalid pattern is a config validation error at load, never a silent
  no-match.
- `only`/`except` intentionally differ from RFC 0011's `include`/`exclude`:
  this is not file selection, and the semantics differ (`all`, union-only
  inheritance, `only` beats `except`). Only the glob engine is shared.
- For args and inputs, a dotted path addresses a nested value
  (`db.password`). Naming a top-level key whose value is an object or array
  redacts every string leaf under it. Dotted paths are mostly useful at task
  level; at workspace level, top-level names and globs are the main use.
- Entries that match nothing are accepted (a value may arrive later through
  `-a`, `-v` or the process environment).

#### Where it can be declared

| Level | File | `args` | `inputs` | `env` |
| --- | --- | --- | --- | --- |
| Workspace | `workspace.omni.yaml` | Every task | Every tool and generator | Every run |
| Project | project config | Every task in the project | not available | Tasks in the project |
| Task | task long form | That task | not available | That task |

`inputs` is workspace-level only: tools and generators are not owned by a
project or task, so there is nowhere narrower to declare it. `redact.args` at
the workspace is a name-only floor (the workspace has no `args` map of its
own) matched against whatever args a task receives; a collision with an
unrelated arg of the same name only over-redacts, never leaks.

Tool and generator inputs also keep their per-input `secret: true`
(`omni_input_schema`). The two sources are unioned; see 4.6.

#### Merge rules (fail closed)

Policies are resolved from workspace to project to task (and through
`extends`) with these rules:

1. `only` lists are **unioned** across all levels. Nothing a child declares
   can un-redact a name an ancestor listed in `only`.
2. `except` lists are unioned, but `only` always wins over `except` for the
   same name, regardless of level.
3. `all` is taken from the nearest level that sets it. A child may switch
   `all: true` to `all: false`, but every ancestor `only` entry still applies
   (rule 1). This lets a task show its env without weakening the names the
   workspace declared.

There is no way to forbid a descendant from setting `all: false`. A `lock`
key for that was considered and deferred (section 8); it can be added later
as a new optional key without breaking existing config.

The resolved policy per category is a pair of matchers plus a default, built
once per task and carried in the task context next to `args`. It is display
metadata only and never enters cache keys; a test pins this.

#### Schema work

- New `RedactConfiguration` in `omni_configurations`, with `RedactPolicy`
  (`all`, `only`, `except`) accepting the list shorthand through an untagged
  enum, plus `mode` at workspace level.
- Field added to workspace, project and long-form task configurations; JSON
  schemas regenerated.
- `args` itself (`DictConfig<DynValue>`) is unchanged. Wrapping each arg as
  `{ value, redact: true }` was rejected: `DynValue` already allows objects,
  so the wrapper is ambiguous, and it would change the type of every arg for
  every consumer.

#### CLI

- `--redact-arg <name>` (repeatable) on the command group that owns
  `-a/--arg` (`common_args.rs`) adds to the `args` `only` list.
- `--redact-input <name>` (repeatable) on `omni tool run` and
  `omni generator run` adds to the `inputs` `only` list.
- `--redact-env <name>` (repeatable) on all three adds to the `env` `only`
  list.
- `--redact <declared|all|none>` on all three selects the mode (4.4).

The flag that supplies a value and the flag that redacts it use matching
words (`-a/--arg` and `--redact-arg`; `-v/--value` and `--redact-input`).
Tool input flags are being renamed to `-v/--value` and `--values-json` in
#51 so tools and generators share one spelling; that change is separate and
is not a prerequisite for this RFC.

There are deliberately no `--except-*` flags: the CLI can tighten, not loosen,
what config declares. Loosening is `--redact none`, which is explicit and
prints a notice.

### 4.3 Shared redactor: the `omni_redact` crate

The redactor lives in a new, standalone `omni_redact` crate so it is not tied
to any specific implementation. It knows nothing about tasks, args, env,
Tera, tracing or diagnostics; it only matches and replaces strings. Callers
resolve the policy and feed it values.

```rust
pub struct Redactor { /* sorted values, placeholder */ }

impl Redactor {
    pub fn builder() -> RedactorBuilder;
    pub fn empty() -> Self;
    /// Replace every sensitive value in `input`; borrows when unchanged.
    pub fn redact<'a>(&self, input: Cow<'a, str>) -> Cow<'a, str>;
    /// Merge two redactors, e.g. workspace-wide plus per-task.
    pub fn union(&self, other: &Redactor) -> Redactor;
}

impl RedactorBuilder {
    pub fn placeholder(self, p: impl Into<String>) -> Self;
    /// Always redacted, however short. For names the user listed.
    pub fn value(self, v: impl Into<String>) -> Self;
    pub fn values<I: IntoIterator<Item = S>, S: Into<String>>(self, v: I) -> Self;
    /// Redacted only when at least `bulk_min_len` long. For values redacted
    /// in bulk (`all: true`), where nobody chose the value individually.
    pub fn bulk_value(self, v: impl Into<String>) -> Self;
    pub fn bulk_values<I: IntoIterator<Item = S>, S: Into<String>>(self, v: I) -> Self;
    /// Minimum length for bulk values. Default 4.
    pub fn bulk_min_len(self, n: usize) -> Self;
    pub fn build(self) -> Redactor;
}
```

Design rules:

- **Dependencies:** `std` and `omni_constants` (for the default placeholder),
  plus optional features. `omni_constants` is dependency-free, so no other
  omni crate is pulled in, and `omni_command_config`, `omni_task_executor`,
  `omni_api`, `omni_mcp_core`, `omni_diagnostic` and `omni_tracing_subscriber`
  can all depend on `omni_redact` without cycles.
- **Optional adapters behind features**, not in the core: a `tracing`
  `Layer`/`MakeWriter` wrapper for log output and a `serde_json::Value`
  walker for JSON payloads. Each is a thin consumer of `Redactor::redact`.
- **Matching rules** carry over from today's `redact_values`: longest first,
  empty values ignored, applied to the unsplit string so values containing
  whitespace match intact, `Cow` borrowed when nothing matches.
- **Short values:** two tiers. Values added with `value` (names the user
  listed in `only`) are always redacted, however short. Values added with
  `bulk_value` (everything redacted because of `all: true`) are skipped when
  shorter than `bulk_min_len` (default 4), so a value like `1`, `true` or
  `ab` does not mangle unrelated text. A user who really wants a short value
  hidden lists its name in `only`. The minimum is an `omni_redact` option;
  omni uses the default and exposes no config key for it.
- **Placeholder** is configurable in the `omni_redact` API
  (`RedactorBuilder::placeholder`), so other consumers can pick their own.
  When none is set, the default is `omni_constants::REDACTED_PLACEHOLDER`.
  omni itself never sets one, so every omni surface gets the same value from
  one place; there is no config key or CLI flag for it. The placeholder is
  also the dry-run indicator that a value was hidden; no separate notice is
  added. Exposing it in config later is additive.
- **Shared constant.** A new `omni_constants::REDACTED_PLACEHOLDER`
  (`"<redacted>"`) is the single source of the literal. `omni_constants` is
  the right home: it is dependency-free and holds values that "must agree
  across subsystems" (`omni_constants/src/lib.rs`). It replaces the private
  `REDACTED_ENV_VALUE` in `omni_task_executor/src/batch_executor.rs`.
- Built once per run (or per task) and shared as `Arc<Redactor>`.
- `redact_values` in `omni_command_config` stays as a thin wrapper over
  `omni_redact` until its callers migrate.

The policy logic (names, globs, `all`, merge rules) lives in
`omni_configurations` / `omni_task_context`, using the name glob matcher
(4.7), not in `omni_redact`. Its
output is just the list of values handed to the builder.

### 4.4 Surfaces and modes

The redactor is applied at these points:

| Surface | Change |
| --- | --- |
| Dry-run command display | Use `Redactor` (replaces the env-only list) |
| `log`/`tracing` output | Redact rendered commands and arg/env values in executor, tool and generator logs |
| Diagnostics (RFC 0021) | Run `DiagnosticReport` text and JSON through the redactor at render time |
| MCP responses | Same renderer path as diagnostics |

`--redact <declared|all|none>` (and a `redact.mode` workspace key):

- `declared` (default): apply the resolved policies.
- `all`: treat every category as `all: true` with no `except` (option 1
  of #41). The bulk minimum length applies.
- `none`: disable redaction for local debugging; prints a one-line notice.
  It is not refused in CI or under any other condition: the notice is the
  guard, and refusing based on environment variables would be surprising.

The mode comes only from config and the process's own CLI flags. MCP tool
parameters cannot set or change it, so an agent cannot turn redaction off or
loosen a policy for a call.

### 4.5 Interaction with templates and `$VAR`

- Redaction stays post-render, as in #40: Tera renders with real values so
  conditionals take the same branches as a real run, then the redactor runs
  on the rendered string before shell splitting.
- `$VAR` references are not expanded in display, so they are always shown as
  references. Env values reach displayed text only through `{{ env.X }}`
  templates (the template context includes `env`) and through log or error
  messages; those are what `redact.env` governs.
- Values transformed by a template filter (for example base64) are not
  matched; documented rather than solved (section 8).

### 4.6 Beyond the task runner

Task `args` and `env` are not the only user-supplied values omni echoes.

| Subsystem | Source of sensitive values | Plan |
| --- | --- | --- |
| Task runner | `args`, `env` | `redact` block (4.2) |
| Tools (`omni tool run`, MCP tool calls) | Inputs with `secret: true`; names in `redact.inputs`; process env | Both sources feed the redactor; workspace `redact.env` applies to the tool's env |
| Generators (CLI and MCP) | Inputs with `secret: true` (password widget); names in `redact.inputs` | Already never persisted; values now also feed the redactor for logs, diagnostics and action output |
| Workspace / project env files | `env.files`, `env.vars` | Covered by `redact.env` at workspace/project level |
| Remote cache | Credentials from env/config | Credential fields are always redacted, not configurable |
| Remote sources | Tokens in git URLs (`https://user:token@...`) | URL userinfo is always redacted in logs and errors, not configurable |

Principles:

- **Reuse existing markers.** Tool and generator inputs already declare
  `secret: true`, documented as "must not be logged, echoed, cached, or
  persisted". This RFC does not add a second marker for them; it makes
  `secret` actually enforced at the output surfaces by feeding those values
  into the same `Redactor`.
- **Two sources, unioned.** A tool or generator input is redacted if its own
  `secret: true` is set (the author's declaration; a `password` widget counts)
  or its name matches `redact.inputs` (the workspace owner's declaration).
  Generators come from remote sources and packs, so an author may forget to
  mark an input, and the workspace owner must be able to fix that without
  forking. Neither source can undo the other.
- **Sub-generators.** `run_generator` passes input values down to
  sub-generators. A value that is secret in the parent stays redacted in
  every nested generator, whatever the child's own schema says.
- **Gaps this closes.** Today generators log only a count of prompt values
  (`run_generator.rs`), but other paths can still echo a value:
  `run_command` actions (the rendered command and errors embedding it),
  `run_javascript` and other custom actions, error messages that format the
  `inputs.*` context, and MCP responses that echo arguments. The
  `log::trace!("resolved target overrides: ...")` in `run_generator.rs` is
  also a possible leak if an override carries an input value. Each is routed
  through the redactor.
- **Non-interactive values.** A value passed with `-v/--value` or MCP
  arguments is only protected if the input is marked `secret` or its name
  matches `redact.inputs`; the second source exists for exactly this case.
- **Naming.** `secret` (inputs) is stronger than `redact` (args/env): a
  secret input is also never cached or persisted, while a redacted arg still
  participates in cache hashing. The two names stay distinct on purpose.
- **Fixed rules for infra credentials.** Remote cache credentials and URL
  userinfo have no legitimate reason to be shown, so they are redacted
  unconditionally and are not part of the `redact` policy.
- **One redactor per run** is built from all of the above and unioned with
  the per-task redactor, so a secret supplied to any subsystem is redacted in
  every surface.

### 4.7 The name glob matcher

Name matching is a small, general facility, not a redaction detail. It lives
as a submodule of `omni_glob` (`omni_glob::name`), kept separate from the
path matcher (`GlobMatcher`) because names are not paths: there is no
`Path`, no root, and the separator is configurable. If `omni_glob` should
stay strictly about paths, the same module moves to a tiny `omni_name_glob`
crate without an API change. `omni_redact` does not depend on it.

#### Syntax

`*` (any run of characters within a segment), `?` (one character), `**` (a
whole segment: zero or more segments), and literals. `\*`, `\?` and `\\`
escape the metacharacters. No character classes, no `!`, no regex (a later
`{ regex: ... }` entry form can be added without touching this syntax).

#### API

Configuration is plain data with defaults; matching goes through one opaque
compiled type.

```rust
pub struct NameGlobOptions {
    pub separator: Separator,        // default: Separator::None
    pub case: Case,                  // default: Case::Sensitive
}

pub enum Separator { None, Char(char) }
pub enum Case { Sensitive, Insensitive }

impl NameGlobOptions {
    pub fn separator(self, s: Separator) -> Self;
    pub fn case(self, c: Case) -> Self;
}

pub struct NameGlobSet { /* opaque */ }

impl NameGlobSet {
    pub fn new<I, S>(patterns: I, options: &NameGlobOptions)
        -> Result<Self, NameGlobError>
    where I: IntoIterator<Item = S>, S: AsRef<str>;

    pub fn is_match(&self, name: &str) -> bool;

    /// Patterns that matched none of `names`; for debug-level typo hints.
    pub fn unmatched<'a, 'n>(&'a self, names: impl IntoIterator<Item = &'n str>)
        -> Vec<&'a str>;
}
```

Rules that keep it configurable and stable:

- **Options are an extensible struct, not positional arguments.** New knobs
  (for example an `escape` character, or `Separator::Chars(&[char])`) are
  added as fields with defaults and builder methods. `NameGlobOptions` and
  both enums are `#[non_exhaustive]`, so adding a field or variant is not a
  breaking change.
- **One entry point, an opaque compiled type.** Callers hold a
  `NameGlobSet` and call `is_match`. They never see the pattern
  representation, so the internals can change freely.
- **The same type serves every kind of name.** Env names and flat names use
  `Separator::None`, args and inputs use `Separator::Char('.')`, and future
  callers pick their own. Defaults are the flat case, which matches how
  project and task name filters behave today.
- **No leaking of the engine.** Errors are `NameGlobError` (opaque kind
  enum, `#[non_exhaustive]`), not `globset::Error`, so the implementation is
  not part of the public API.

#### Typo hints

A name that matches no pattern is accepted by design (a value may arrive
later through `-a`, `-v` or the environment), so a misspelled entry
(`pasword`) silently does nothing. To make that visible, after policy
resolution omni logs, at debug level, every `only` and `except` pattern that
matched no name in the run (`NameGlobSet::unmatched`). It is a hint in the
debug log only, never an error and never printed by default.

#### Implementation, and room to optimize

v1 is deliberately simple: patterns are parsed into segments of literal and
wildcard tokens; matching is an iterative two-pointer scan per segment with
no backtracking blow-up; wildcard-free patterns go into a hash set for O(1)
lookup. This is simple and has no path assumptions. It is not claimed to be
faster than `globset`, which groups patterns by shape and scales better with
hundreds of patterns; at our scale (a handful of names, matched once per
task) neither matters.

Because the compiled form is opaque, later optimizations need no API change:

- bucket patterns by shape (prefix, suffix, contains) with a literal prefilter
  or Aho-Corasick for large sets;
- precompute a per-set first-byte or length filter to reject names cheaply;
- cache compiled sets by `(patterns, options)` the way RFC 0007 memoizes
  path globs;
- swap the engine for `globset` or `regex` internally if a profile ever
  justifies it.

A shared test suite (`tests/name_glob_cases`) holds the behavior as data
(pattern, options, name, expected). Any implementation must pass it, and
changes to the engine are validated by running the same cases through old
and new.

#### Future uses (out of scope here)

The same module fits other name-based filtering already in the tree, each as
a separate change:

- the project and task name filters (`omni_execution_plan/src/filter.rs`,
  currently `globset` over names with `*` crossing `/` and `:`), using
  `Separator::None` so behavior is identical. A migration must run the
  existing filter tests through both implementations;
- `cache.env` (env var names hashed into cache keys), which is exact names
  today. Supporting globs there changes hashing, because a glob must be
  resolved against the real environment, so it needs its own RFC.

### 4.8 Limitations

Redaction is a safety net, not a guarantee. Stated plainly:

- **Lists cannot cover names nobody thought of.** In opt-in mode
  (`all: false`, the default for args and inputs), a value whose name matches
  no entry is shown. A newly added `GITHUB_PAT` is not covered until someone
  lists it. For CI and shared logs, set `all: true`.
- **Matching is on names, not values.** A secret under an unexpected key, or
  embedded in a larger value (a URL with credentials, a JSON blob), is not
  caught unless its key is listed; for object values, only string leaves of a
  listed key are.
- **Transformed values are not matched** (4.5), and child process output is
  not redacted (section 3).
- **A misspelled name does nothing** (it only shows up in the debug hint
  above).
- **Case differs by platform:** env names are case-insensitive on Windows and
  case-sensitive elsewhere, so a config written on Windows can under-redact on
  Linux. Write env names in their canonical case.

## 5. Testing

- Name glob: the shared case table (separators `None` and `.`, case modes,
  `*`/`?`/`**`, escapes, empty names and patterns, invalid patterns),
  `unmatched`, and that `NameGlobSet` is unchanged by adding an option
  (non-exhaustive structs).
- Unit tests for `omni_redact`: longest-first, overlap, whitespace values,
  empty values, borrow when unchanged, `union`, `value` always redacted when
  short, `bulk_value` skipped below `bulk_min_len` and redacted at it.
- `all: true` with short unlisted env values (`1`, `true`) leaves them
  visible; the same names listed in `only` are redacted.
- `--redact none` prints the notice and is not refused under `CI`; MCP tool
  parameters cannot change the mode.
- Debug log lists patterns that matched nothing in a run.
- Policy resolution tests: shorthand vs long form, globs, dotted arg paths,
  `only` beats `except` across levels, `all` from nearest level, cache
  keys unchanged.
- Update the e2e test `-d/--dry-run prints the command with dynamic args
  expanded` to use an unlisted argument, and add cases for a redacted arg,
  `--redact-arg`, `--redact-input`, `redact.inputs` combined with per-input
  `secret`, sub-generator propagation, env `all: true` with `except`, env `all: false` with `only`, a
  bare list not changing an inherited `all`, and each
  `--redact` mode.
- Tool and generator tests: a `secret: true` input value, and a value whose
  name matches `redact.inputs`, never appears in logs, `run_command` output,
  diagnostics or MCP error payloads, including in a nested sub-generator.
- Log capture test asserting a declared secret never appears in debug log
  output of a dry-run and a real run.

## 6. Compatibility

- Defaults are unchanged: env stays redacted in dry-run, args stay shown.
  One deliberate difference: env values shorter than 4 characters that are not
  named in `only` are no longer redacted under `all: true` (4.2), so values
  like `1` and `true` stop corrupting unrelated text. Name them in `only` to
  keep hiding them.
- The other visible change by default is that `secret` tool and generator
  inputs, remote cache credentials and URL userinfo are now redacted where
  they previously could leak.
- Config schema gains an optional `redact` block at three levels, so
  existing configs stay valid; JSON schemas are regenerated.
- No change to execution, hashing or cache keys: the policy is display
  metadata and is not hashed. A `redact` block written in a project config file
  is part of that file, which is already a cache key input, so editing it
  invalidates that project's cache like any other edit to the file. A
  workspace-level `redact` block does not change project hashes.

## 7. Rollout

1. Add `omni_redact`, `omni_constants::REDACTED_PLACEHOLDER` and the
   `omni_glob::name` matcher, and move the dry-run
   path onto them with the current env-only behavior.
2. Add the `redact` block, policy resolution and merge rules, plus
   `--redact-arg`, `--redact-env` and `--redact`. Fixes #41.
3. Add `redact.inputs` and `--redact-input`, and feed `secret` tool and
   generator inputs (with sub-generator propagation), remote cache credentials and URL
   userinfo into the run-wide redactor.
4. Wire logging/tracing and generator action call sites.
5. Wire RFC 0021 renderers.
6. Documentation page for redaction behavior and its limits.

### Implementation status

Steps 1 to 3 landed together, plus the parts of 4 and 5 that have a call site
in this tree:

- Dry-run command display, generator `transform` / `transform-many` /
  `run-command` logs and errors, the generator input-collection debug logs,
  tool validation and run errors, RFC 0021 diagnostics (text and JSON, CLI and
  MCP) and the git clone trace log are routed through the redactor or the fixed
  URL-userinfo rule.
- `--redact`, `--redact-arg`, `--redact-env` (`run`, `exec`),
  `--redact-input`, `--redact-env`, `--redact` (`generator run`) and
  `--redact-input`, `--redact` (`tool run`) are available. MCP never exposes
  them.
- Not done, with the reason:
  - **Generic `tracing` / `log` adapters** (the optional features in 4.3): no
    call site outside those above echoes a sensitive value today, so a global
    log filter would add cost without closing a known leak. It can be added
    behind the existing `Redactor` API.
  - **Remote cache credentials**: the remote cache configuration in this tree
    holds no secret fields (credentials come from the process environment and
    are never formatted), so there is nothing to redact yet.
  - **Workspace `redact.env` applied to a tool's env**: tool runs do not
    format env values, so this has no visible effect yet.
  - **Documentation page** (step 6).

## 8. Open questions

None remaining. Decisions taken while drafting:

- **Crate placement:** standalone `omni_redact` (4.3).
- **Marking args:** a sibling `redact` block, not wrapped values (4.2).
- **Heuristics:** dropped (4.1).
- **Block name:** `redact`, not `sensitive` (4.2).
- **Bare-list shorthand:** adds to `only`, never changes `all` (4.2).
- **Tools and generators:** no `redact` block of their own; `redact.inputs`
  plus per-input `secret` (4.2 and 4.6).
- **`args` vs `inputs`:** separate keys mirroring each concept (4.2).
- **Transformed values:** not matched. A value run through a template filter
  such as base64 cannot be solved in general; documented in 4.5.
- **Placeholder / redaction indicator:** `<redacted>`, the default from
  `omni_constants::REDACTED_PLACEHOLDER`; configurable only through the
  `omni_redact` API (4.3).
- **Short values:** two tiers. Listed names are always redacted; values
  redacted only because of `all: true` are skipped below 4 characters
  (4.2, 4.3).
- **`--redact none` in CI:** notice only, never refused (4.4).
- **MCP override:** not possible; the mode never comes from tool parameters
  (4.4).
- **Typo hints:** debug-level log of patterns that matched nothing (4.7).
- **Child process output:** out of scope for v1 (section 3).
- **`redact.lock`:** deferred. The fail-closed merge rules already stop
  descendants weakening `only`, and a lock can be added later as a new
  optional key.
