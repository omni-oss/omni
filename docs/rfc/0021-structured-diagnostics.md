# RFC 0021: Structured diagnostics for CLI and MCP error reporting

- **Status:** Draft <!-- Draft | Accepted | Implemented | Superseded -->
- **Owner:** TBD
- **Date:** 2026-11-03
- **Base branch:** `main`
- **Affected crates/files:** new `omni_diagnostic` and `omni_diagnostic_derive` crates (the derive, the `Diagnostic` trait, `Audience`, `DiagnosticReport`, and the text/JSON renderers); `omni_execution_plan` (`meta_filter_error.rs` becomes the first implementor); `omni_task_executor` (`TaskExecutorError`); `omni_expressions` (`ErrorClass`); `omni_api`; `omni_cli_core`; `cli/omni` (`lib.rs`, top-level error handling); `omni_mcp_core` (`error.rs`, `server.rs`); `tests/omni-tests`
- **Follows:** RFC 0020 (meta filter runtime errors)

## 1. Summary

RFC 0020 made meta filter failures visible with an aggregated, structured
diagnostic. It also had to hand-wire that diagnostic through every layer: a
bespoke `MetaFilterError` type, a bespoke accessor on `TaskExecutorError`, a
bespoke `downcast_ref` branch in the MCP server, a bespoke `render_warning`,
and a bespoke `to_json`. None of that is reusable. The next subsystem that
wants a good error has to repeat all of it, and until then it gets the generic
path, which is poor on both surfaces.

This RFC proposes a small, shared **diagnostic model**: one trait that an
error type implements to describe itself (stable code, audience, summary,
structured details, hints), and one set of renderers that turn any diagnostic
into CLI text or an MCP error payload. Subsystems opt in by implementing the
trait; everything else keeps working unchanged through `eyre`. The meta
filter error is migrated first as proof, with no change in user-visible
behaviour.

This is deliberately **not** a rewrite of error handling across the workspace.
It is an additive layer at the two boundaries (CLI exit and MCP response) plus
an opt-in trait.

## 2. Motivation / Problem

### 2.1 Where errors go today

Findings from the current tree (`main` @ `6441fa7b`):

- **Internal errors are `eyre::Report` / `thiserror` enums.** Roughly 17
  crates use the same `Error(ErrorInner)` + `ErrorKind` (strum discriminants)
  pattern, and about 119 call sites use `bail!`/`eyre!`/`Err(eyre...)`. Errors
  are mostly strings by the time they leave a crate.
- **CLI:** `cli/omni/src/lib.rs` installs `color_eyre`, and `main` returns
  `eyre::Result<()>`. Every failure, expected or not, is printed as an
  `eyre` report (with a location section in debug builds). There is no
  distinction between "you passed a bad filter" and "omni has a bug", no
  stable exit codes beyond success/failure, and no machine-readable output.
- **MCP:** `omni_mcp_core/src/server.rs` maps failures to
  `ErrorData::internal_error(e.to_string(), None)`. Two problems follow:
  1. `e.to_string()` drops the `eyre` cause chain (only the outermost message
     survives; the CLI's `{:#}` would have kept it). An agent sees
     `failed to plan tasks` and not why.
  2. Everything is `internal_error` (-32603), so an agent cannot tell "fix
     your arguments and retry" from "the server is broken".
- **Special cases are hard-coded.** `call0` in `server.rs` contains one
  `downcast_ref::<TaskExecutorError>().and_then(|e| e.meta_filter_error())`
  branch. It is the only `downcast_ref` for domain errors in the workspace,
  and it exists solely for RFC 0020. Adding a second structured error means
  adding a second branch to a function that knows about every subsystem.
- **Tolerated problems are strings.** Non-fatal problems go through
  `log::warn!` with a pre-rendered string (`MetaFilterError::render_warning`).
  MCP clients never see them. Non-fatal reports are out of scope here (4.5).
- **`omni_mcp_core::Error` is nearly empty.** It has a single `Custom` variant
  and `into_mcp_error` always produces `internal_error`.

### 2.2 What this costs

- An agent calling an MCP tool cannot self-correct on most failures. RFC 0020
  fixed this for one error; invalid project names, bad task selectors, cache
  config errors, and generator input validation still return a flat string.
- Each new structured error costs a type, a JSON shape, a renderer, a
  downcast, and tests, duplicated per subsystem. In practice that cost means it
  does not get paid.
- CLI and MCP drift. The text for `--meta` failures and the JSON for the same
  failure are produced by two separate functions that must be kept in sync by
  hand.
- Users cannot script against failures. There is no stable code to match, and
  the exit status is always `1`.

### 2.3 Reproduction

```text
$ omni run nonexistent-task
Error: no tasks matched                 # generic; no code, no hint, exit 1

# MCP: task_run with a bad project name
{ "code": -32603, "message": "failed to resolve project filter" }
#   internal_error, cause chain dropped, nothing structured to act on
```

## 3. Goals / Non-goals

### Goals

- One way for an error type to describe itself: stable code, audience,
  one-line summary, structured details, optional hints, optional source
  location.
- One renderer per surface (CLI text, MCP JSON) that works for **any**
  implementor, so adding a diagnostic never touches `server.rs` or `main`.
- MCP errors carry the right JSON-RPC class (`invalid_params` for user
  mistakes, `internal_error` for bugs) and keep the full cause chain.
- Incremental adoption. Un-migrated errors keep working and are rendered at
  least as well as today.
- RFC 0020's observable output (text and JSON) is preserved bit-for-bit where
  the RFC specified it, apart from the additions below.

### Non-goals

- Replacing `eyre`/`thiserror` or converting every error in the workspace.
- Source-span rendering (`miette`-style underlines) in v1. The model leaves
  room for it (section 4.6) but does not require it.
- Localisation.
- Changing exit status semantics for successful runs, or for the exit code
  of a task process that omni runs and forwards.
- Tracing/log output format (`log`/`tracing` are unchanged).
- Non-fatal diagnostics (warnings) and their delivery to MCP. See 4.5.
- Redefining notices. Tracked in omni-oss/omni#47.

## 4. Design

### 4.1 The `Diagnostic` trait

A new leaf crate, `omni_diagnostic`, with no dependency on any other omni
crate (so every crate may depend on it without creating cycles):

```rust
pub trait Diagnostic: std::error::Error + Send + Sync + 'static {
    /// Stable, dotted, lowercase identifier. Part of the public interface.
    fn code(&self) -> &'static str;

    /// Who is expected to act on this. Drives MCP error class and exit code.
    fn audience(&self) -> Audience { Audience::User }

    /// Structured payload. `None` means "message only".
    fn details(&self) -> Option<serde_json::Value> { None }

    /// Actionable suggestions, rendered after the body.
    fn hints(&self) -> Vec<String> { Vec::new() }

    /// Extra text rendered between the summary and the hints (CLI only).
    /// Default: nothing. MetaFilterError uses this for its grouped body.
    fn render_body(&self, _out: &mut dyn fmt::Write) -> fmt::Result { Ok(()) }
}

pub enum Audience {
    /// The input is wrong: arguments, config, expression, files.
    User,
    /// Environment or external system: network, disk, permissions.
    Environment,
    /// omni itself misbehaved.
    Internal,
}
```

`Display` (from `std::error::Error`) is the **summary line** and must be a
single line. Bodies and hints are separate so each surface can lay them out.

Codes are namespaced by subsystem and stable once released:

```text
meta_filter.evaluation          project_filter.unknown_project
task.not_found                  cache.backend_unreachable
config.invalid                  generator.input_invalid
```

Codes are listed in one table in the docs site
(`apps/site/content/docs/references/diagnostics`). Adding one is a docs change;
renaming one is a breaking change.

### 4.2 Carrying diagnostics through `eyre`

Diagnostics travel as ordinary errors, so no signature changes are needed.
At the boundary a single helper finds the diagnostic inside a `Report`:

```rust
pub fn find(report: &eyre::Report) -> Option<&dyn Diagnostic>;
```

It walks `report.chain()` and returns the **outermost** error that is a
diagnostic. Crates register their types once so the lookup does not need to
know every type:

```rust
omni_diagnostic::register!(MetaFilterError);
```

Implementation note: `dyn Error` cannot be downcast to `dyn Diagnostic`
directly, so each implementor exposes itself through a small
`as_diagnostic(&self) -> Option<&dyn Diagnostic>` hook on a thin wrapper
(`DiagnosticError<E>`) that crates use as their inner variant, or via
`eyre::Report::new(DiagnosticError(e))`. The final mechanism is an open
question (section 8) because it affects how invasive adoption is; the
requirement is that `find` works for an error wrapped by `.wrap_err(...)`
layers added by callers.

Context added with `wrap_err` above a diagnostic is preserved as a `context`
array in both renderers (section 4.4), outermost first. This is what keeps the
cause chain that MCP currently loses.

### 4.3 Surfaces

#### CLI

`cli/omni/src/lib.rs::main` handles the top-level error:

1. `find(&report)` returns a diagnostic: render it with the text renderer
   (4.4), exit with the code for its `Audience`.
2. No diagnostic: fall through to the existing `color_eyre` report. Behaviour
   is unchanged.

`color_eyre` stays installed for panics and for un-migrated errors.
The location section stays debug-only.

Exit codes (new, documented):

| Code | Meaning                                              |
| ---- | ---------------------------------------------------- |
| `1`  | Generic failure (unchanged default, un-migrated)     |
| `2`  | `Audience::User` (bad arguments/config/expression)   |
| `3`  | `Audience::Environment` (network, filesystem, auth)  |
| `70` | `Audience::Internal` (`EX_SOFTWARE`)                 |

The exit status of a forwarded task process is never remapped.

A `--error-format <text|json>` global option (and `OMNI_ERROR_FORMAT`) selects
the renderer. `json` prints one JSON object to stderr (the same object MCP
returns in `data`, section 4.4), for scripts and CI annotators. Default is
`text`.

Colour and unicode follow the existing terminal detection; piped stderr gets
plain text.

#### MCP

`call0` (and the other `Err(e)` sites in `server.rs`) replace the
meta-filter-specific branch with:

```rust
fn to_mcp_error(report: eyre::Report) -> ErrorData {
    match omni_diagnostic::find(&report) {
        Some(d) => ErrorData::new(code_for(d.audience()), d.to_string(), Some(render_json(&report, d))),
        None => ErrorData::internal_error(format!("{report:#}"), Some(chain_json(&report))),
    }
}
```

- `Audience::User` maps to `invalid_params` (-32602). Environment and Internal
  map to `internal_error` (-32603). The `data.audience` field lets an agent
  distinguish the latter two.
- The fallback now uses `{report:#}` plus a `context` array, so un-migrated
  errors stop dropping their cause chain. This alone is an improvement for
  every existing tool.
- `omni_mcp_core::Error::into_mcp_error` is updated to the same function.

### 4.4 Rendered shapes

JSON (`data` in MCP, stderr in `--error-format json`). The envelope is fixed;
`details` is per-code:

```json
{
  "kind": "meta_filter.evaluation",
  "audience": "user",
  "message": "meta filter failed to evaluate for 500 task(s)",
  "context": ["planning tasks for `test`"],
  "details": { "filter": "knd == \"e2e\"", "total": 500, "groups": [ "..." ] },
  "hints": ["`knd` is not defined for this task. Did you mean `kind`?"]
}
```

For compatibility with RFC 0020 clients, `data.kind` for the meta filter
changes from `"meta_filter_evaluation"` to `"meta_filter.evaluation"` and the
existing `filter`/`total`/`groups` fields are **also** emitted at the top level
of `data` for one release (marked deprecated in the docs), then removed. This
is the only intentional wire change.

Text:

```text
error[meta_filter.evaluation]: meta filter failed to evaluate for 500 task(s)
  while planning tasks for `test`

  <body from render_body, exactly the RFC 0020 layout>

  hint: `knd` is not defined for this task. Did you mean `kind`?
```

The `[code]` suffix is new and is the only change to the RFC 0020 text layout.
Tests that assert on substrings from RFC 0020 are unaffected.

### 4.5 Non-fatal reports (out of scope)

`Diagnostic` describes **errors** only. v1 has no `Severity`, no
`emit_warning`, and no MCP warning list: the only non-fatal case today
(`--meta-on-error exclude`) stays a `NoticeLevel::Warn` notice with its
details in `fields`, exactly as it is logged today.

Adding warnings later is additive: `severity()` would be a defaulted trait
method returning `Error`, with a matching derive attribute and an optional
`severity` field in the JSON envelope, so no existing diagnostic or client
breaks. It is deferred until there are several real use cases, so severity
and delivery can be designed together. Delivery to MCP overlaps with the
notices redesign in omni-oss/omni#47.

### 4.6 Locations (reserved)

`Diagnostic` may later gain `fn location(&self) -> Option<Location>` where
`Location { file: PathBuf, line: u32, column: u32, len: u32 }`, used for
config-file errors. v1 does not render underlines. The JSON envelope reserves
an optional `location` field so adding it is not a wire break.

### 4.7 The `Diagnostic` derive

`#[derive(Diagnostic)]` ships in v1, in a proc-macro crate
`omni_diagnostic_derive`, re-exported as `omni_diagnostic::Diagnostic`. It
**complements** `thiserror`: `thiserror` still owns `Display`/`Error`/`#[from]`;
the derive owns only the `Diagnostic` impl and its registration. Replacing
`thiserror` is a non-goal.

```rust
#[derive(Debug, thiserror::Error, Diagnostic)]
#[diagnostic(audience = user)]                        // type-level defaults
pub enum ProjectFilterError {
    #[error("unknown project `{name}`")]
    #[diagnostic(
        code = "project_filter.unknown_project",
        hint = "run `omni projects` to list projects",
        hint = "did you mean `{suggestion}`?",         // only if `suggestion` is a field
    )]
    UnknownProject {
        #[diagnostic(detail)] name: String,            // goes into `details`
        #[diagnostic(detail)] suggestion: String,
    },

    #[error("cannot read workspace config")]
    #[diagnostic(code = "config.unreadable", audience = environment)]
    Io(#[source] std::io::Error),

    #[error(transparent)]
    #[diagnostic(forward)]                             // delegate every method
    Meta(#[from] MetaFilterError),
}
```

**Attributes**

| Where | Attribute | Effect |
| --- | --- | --- |
| type or variant | `audience = user\|environment\|internal` | Variant overrides type default. Type default is `user`. |
| variant or struct | `code = "a.b"` | **Required** (see rules). Never derived from the name. |
| variant or struct | `hint = "fmt"` | Repeatable. Same `{0}` / `{field}` / `{field:?}` interpolation as `thiserror`. |
| field | `detail` / `detail = "key"` | Collected into the `details` JSON object (field must be `Serialize`). Key defaults to the field name. |
| variant or struct | `details = path::fn` | Escape hatch: `fn(&Self) -> serde_json::Value`. Mutually exclusive with field `detail`. |
| variant or struct | `body = path::fn` | Escape hatch for `render_body`. |
| variant | `forward` | Variant has exactly one field implementing `Diagnostic`; all methods delegate to it. |
| type | `no_register` | Skip registration (for diagnostics only ever constructed by hand). |

**Generated code**

- `impl Diagnostic` with `code`, `audience`, `details`, `hints`
  matching per variant. Omitted attributes fall back to the trait defaults.
- Registration for `find` (4.2) via `inventory`, including per-variant metadata
  (`code`, `audience`). Types with generic parameters cannot be
  registered and fail with a clear error (use `no_register` or a concrete
  wrapper).

**Compile-time rules** (errors point at the attribute span)

- Every variant needs `code` or `forward`; a missing one is an error, so a new
  variant cannot silently become uncoded.
- Codes must match `^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$`.
- Codes are unique within the type.
- Every `{field}` in a `hint` must exist on that variant.
- `detail` and `details =` cannot be combined; `forward` takes no other
  attributes.
- The type must implement `std::error::Error` (guaranteed by the trait's
  supertrait bound, so a missing `thiserror` derive gives a normal bound error).

**Cross-crate checks.** Uniqueness across crates cannot be seen by a
proc-macro, so `omni_diagnostic::testing::assert_registry_valid()` iterates the
registry and checks code format and global uniqueness. It runs in the `omni`
binary's test suite, where every crate is linked. The same registry backs
`omni diagnostics list [--format json]` and generates the docs code table
(answers open question 5, no hand-maintained table to drift).

**Design constraints**

- **Explicit over inferred.** No code from type names, no audience guessing.
  Public identifiers should never change by renaming a variant.
- **Attribute-only.** No new traits users must learn beyond `Diagnostic`.
- **Escape hatches are plain functions**, so complex cases (`MetaFilterError`'s
  grouped body) need no macro features.
- **Testing:** `trybuild` UI tests for every compile-time rule above, and
  expansion tests for struct, enum, `forward`, and generic-type cases.
- **Dependencies:** `syn`, `quote`, `proc-macro2`; attribute parsing via
  `darling` unless review prefers hand-rolled parsing to keep build time down.

### 4.8 What does **not** change

- Library crates keep returning their own `Error`/`eyre::Result`. No
  `Result` signature changes.
- Un-migrated errors render exactly as today on the CLI, and strictly better
  on MCP (full chain).
- `ErrorKind` strum discriminants stay as the in-process classification;
  `code()` is the external, stable one. They are intentionally separate so
  refactors do not change public codes.

### 4.9 Naming: `Diagnostic` versus `Notice`

Before this RFC, `omni_messages` had a subscriber channel called
`DiagnosticEvent` / `DiagnosticLevel` / `DiagnosticSubscriber`. To free the
`diagnostic` name for the model above, that channel is renamed to
`NoticeEvent` / `NoticeLevel` / `NoticeSubscriber` (`on_notice`,
`wants_notices`, `notice!`, `notice_event!`) as a mechanical first step,
separate from this RFC's implementation. `NoticeLevel` has only `Info`,
`Warn` and `Error`; developer-oriented trace/debug output uses `log::*`.

The two are distinct:

- A **notice** is a leveled, free-text runtime message pushed to
  subscribers while work is happening. It has no code, audience or details.
- A **diagnostic** is a structured, coded error that describes a specific
  problem and is rendered at the CLI/MCP boundary.

What a notice means across the CLI and MCP, and how notices are exposed on MCP,
is a separate piece of work: omni-oss/omni#47.

## 5. Alternatives considered

1. **Adopt `miette`.** It provides codes, help text, related errors, and span
   rendering, so it is the closest off-the-shelf fit. Rejected for v1: it
   would replace `color_eyre` (a workspace-wide decision), its JSON output is
   not designed for MCP payloads, and our primary need is aggregated
   structured details, not source spans. The trait in 4.1 is intentionally
   close to `miette::Diagnostic` so a later switch is mechanical. Worth
   revisiting when config-file span errors become a goal.
2. **One big `OmniError` enum** in a shared crate. Gives exhaustive matching
   but forces every crate to depend on a crate that knows every subsystem, and
   it fights the existing per-crate `Error(ErrorInner)` pattern.
3. **Keep adding bespoke downcasts** (the RFC 0020 approach). Cheapest per
   error, worst in aggregate; `server.rs` becomes a registry of subsystems.
4. **Only fix MCP** (use `{:#}` and a better error class). Worth doing
   regardless and included here, but does not give agents structured details
   or CLI users scriptable output.

## 6. Risks

- **Two error systems.** Mitigated by the diagnostic being a trait on
  existing error types, not a parallel type hierarchy, and by the `find`
  fallback making migration optional.
- **Wire stability.** Codes and the JSON envelope become public. Mitigated by
  the docs table, a snapshot test per migrated code, and the one-release
  compatibility fields in 4.4.
- **Exit code change.** Scripts that check `$? -eq 1` for user errors will see
  `2` for migrated diagnostics. Mitigated by migrating only the meta filter
  error initially (it is a few days old) and calling this out in the changelog
  before more codes adopt it.
- **Downcast mechanism.** See section 8.

## 7. Implementation plan

Each step is independently shippable.

1. **`omni_diagnostic` and `omni_diagnostic_derive` crates** (4.7). Trait, derive, `Audience`, `find`, text and
   JSON renderers, unit tests including `wrap_err` layering. No consumers yet.
2. **MCP fallback fix.** `to_mcp_error` with `{:#}` and the `context` array
   for un-migrated errors. Replaces the `meta_filter_error()` branch with the
   generic path once step 3 lands; until then keep both. Highest value, lowest
   risk.
3. **Migrate `MetaFilterError`.** Derive `Diagnostic` (with `details =` and `body =` escape hatches)
   (`meta_filter.evaluation`, `Audience::User`), delete `render_warning`/
   `to_json`/`TaskExecutorError::meta_filter_error` in favour of the trait,
   and keep the deprecated top-level JSON fields. Existing RFC 0020 tests must
   pass unchanged apart from the `[code]` suffix and `kind` value.
4. **CLI boundary.** `main` renders diagnostics, adds `--error-format` and exit
   codes, documents them.
5. **Next migrations (separate PRs, one subsystem each).** Candidates in
   priority order, chosen because an agent or CI user hits them often:
   unknown project/task in filters, invalid `workspace.omni.yaml` / project
   config (with `location` in a later RFC), generator input validation, cache
   backend errors.

## 8. Open questions

1. **Downcast mechanism (4.2).** Options: (a) a `register!` macro building a
   `TypeId -> fn(&dyn Any) -> &dyn Diagnostic` table; (b) require diagnostics
   to be constructed via `DiagnosticError<E>`; (c) a blanket `trait
   AsDiagnostic` implemented through `eyre`'s `Report::downcast_ref` for each
   known type behind a feature. Preference: (a), since it keeps existing
   `thiserror` enums untouched. Needs a short prototype to confirm it works
   through `wrap_err` layers.
2. **Exit codes.** Are `2`/`3`/`70` right, or should omni stay at `1` and only
   add `--error-format json`? Exit codes are the most visible compatibility
   surface here.
3. **Non-fatal diagnostics.** Deferred (4.5). Revisit with severity and MCP
   delivery together, alongside omni-oss/omni#47.
4. **Notices on MCP.** Out of scope; tracked in omni-oss/omni#47.
5. **Where does the code table live?** The docs site (proposed) versus a
   generated file from `register!` so it cannot drift from the source.
6. **Scope of v1.** This RFC proposes the model plus the meta filter
   migration. If preferred, it can be narrowed to the execution-plan and
   task-executor subsystems only, leaving the CLI boundary for later.
