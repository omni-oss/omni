# RFC 0026: Operation-scoped redaction

- **Status:** Implemented, with the deviations in section 8 <!-- Draft | Accepted | Implemented | Superseded -->
- **Owner:** TBD
- **Date:** 2026-11-08
- **Base branch:** `main`
- **Affected crates/files:** `omni_messages` (`RedactingSubscriber`, subscriber traits, event sanitization, notice publishing and tracing); `omni_redact`; `omni_task_executor`; `omni_generator`; `omni_api` (tool and generator operations); `omni_input_provider` / `omni_input_schema` (collection and validation); `omni_diagnostic` (safe error projections); `omni_cli_core` and `omni_mcp_core` (presenters and result/error exits); integration tests
- **Follows:** [issue #58](https://github.com/omni-oss/omni/issues/58), RFC 0022 (redaction), RFC 0023 (events, notices and tracing), implemented in PR #55

## 1. Summary

Replace subscriber-owned, growing secret sets with an **operation-scoped
redacting subscriber** (`RedactingSubscriber`) holding an immutable `Arc<Redactor>`. Producers sanitize output
before it reaches a subscriber, channel, tracing mirror, or surface adapter.
Presenters retain only sanitized output, never operation redactors.

A child inherits a snapshot of its parent's secrets and adds its own. It
does not mutate its parent or siblings. Independent tasks, generator runs,
tool calls and later operations may share a subscriber without sharing
concrete secret values.

This RFC proposes a design only. Implementation follows on the same branch
after agreement; it does not request changes to PR #55.

## 2. Motivation and current behavior

`omni_messages::SharedRedactor` uses a mutex around an `Arc<Redactor>`.
`NoticeSubscriber::extend_redactor` unions additional values into it.
`CliSubscriber` and `McpSubscriber` keep that union for their lifetime.

- `omni_task_executor/src/batch_executor.rs::build_redactor` registers each
  task's args/env values with the shared subscriber.
- `omni_generator/src/run.rs::run_internal` registers collected generator
  inputs there too. Nested generators also explicitly inherit a redactor.
- `omni_messages/src/publish.rs::publish_event` reads the subscriber's
  redactor before mirroring a notice to tracing and delivering it.
- Generator lifecycle and script output have additional local/presenter
  redaction paths.
- `omni_api/src/operations/tool.rs::handle_tool_run` instead builds a local
  input redactor, sanitizes forwarded notices and some errors, and leaves
  the tool's successful JSON return value untouched.

Updates are synchronized, and the state is not process-global. The problem
is ownership: a shared output consumer outlives the operations supplying its
secrets. Concurrent operations over-mask one another, reuse retains old
values, and late registration cannot protect output already dispatched.

## 3. Goals and boundaries

**Goals**

- One ownership and publication contract for tasks, generators and tools.
- Isolation independent of scheduling, subscriber reuse, and CLI/MCP surface.
- Parent-to-child inheritance without upward or sideways accumulation.
- Protection before every governed output exit, including custom subscribers
  and tracing subscribers, success and failure.
- Release redactor references when their owning operation finishes or is
  cancelled, rather than when its presenter is dropped.

**Non-goals**

- Changing policy syntax, modes, matching rules, placeholders, cache keys,
  execution inputs, or the `Diagnostic` trait.
- Scrubbing arbitrary child-process stdout/stderr, cached task byte streams,
  generated file contents, or secret transformations. RFC 0022's limitations
  remain; this is not a new promise to sanitize all bytes a program produces.
- A global tracing layer or task-local secret registry. Arbitrary direct
  tracing fields are not made safe by installing a redacting subscriber.
- Guaranteed memory zeroization. Dropping a redactor bounds ownership, not
  allocator residency, copies in execution inputs, or process crash dumps.

## 4. Design

### 4.1 Ownership and scope tree

An operation is an invocation, not a subscriber and not a task identifier:

| Scope                | Owns                                                                        | Inherits from                             |
| -------------------- | --------------------------------------------------------------------------- | ----------------------------------------- |
| Request/run root     | Explicit common values actually available to that run                       | Nothing from previous requests            |
| Task invocation      | Resolved task args/env redactor; retries may reuse it                       | Its run root, not prerequisite tasks      |
| Generator invocation | Supplied, defaulted and prompted input redactor, plus applicable env policy | Its enclosing generator or request root   |
| Tool invocation      | Supplied/defaulted input and applicable env redactor                        | Its explicit caller scope or request root |

Workspace **policy** is reusable metadata. It is not a reason to retain a
workspace-wide set of concrete values. Root scopes contain only genuinely
shared values, not the union of descendant task contexts.

For parent `P` and child-local values `L`, the child's effective immutable
redactor is `P.union(L)`. All snapshots are immutable. New values produce a
new scope; they never update an existing snapshot. The child keeps inherited
protection even if its local policy is weaker. Mode resolution remains
server/CLI-owned; MCP arguments cannot change it. Existing `none` behavior
and its essential notice remain, without disabling inherited protection.

Creating two children from the same parent yields two independent snapshots.
There is no implicit scope inheritance through ambient thread state or a
shared subscriber. Spawned work must explicitly receive its owning scope.

### 4.2 Redacting subscriber and sink responsibilities

Add a `RedactingSubscriber` in `omni_messages`, borrowing the destination
subscriber and owning an `Arc<Redactor>`. The following is an API sketch, not
a requirement to introduce dynamic subscriber dispatch:

```rust,ignore
pub struct RedactingSubscriber<'a, S> {
    sink: &'a S,
    redactor: Arc<Redactor>,
}

impl<'a, S> RedactingSubscriber<'a, S> {
    pub fn new(sink: &'a S, redactor: Arc<Redactor>) -> Self;
    pub fn child(&self, local: &Redactor) -> RedactingSubscriber<'a, S>;
    // Typed publishing methods sanitize, then forward to `sink`.
}
```

`child` retains the same underlying `S`, not a recursively nested wrapper
type. This matters for recursive generator calls and existing generic,
async subscriber traits. It must not reveal the raw sink to producer code.

The redacting subscriber exposes the existing demand checks and typed emission methods
needed by the execution and generator producers. Subscriber traits remain
the sink interface. Producer configurations and handler contexts receive a
redacting subscriber rather than an unrestricted subscriber for output publication.

Each governed message follows exactly one path:

1. Construct the raw event inside the owning operation.
2. Sanitize its governed fields with the operation snapshot.
3. Mirror a notice to tracing using only that sanitized event.
4. Dispatch the sanitized event to the subscriber.

Refactor `notice!`, `diagnostic_notice!`, `publish_event` and `publish_batch`
to target this boundary. Keep lazy construction and batched delivery/demand
semantics. Direct sink callbacks are delivery APIs, not producer APIs.
Use a private prepared-event representation internally where needed to
prevent double sanitization or duplicate tracing mirrors. Redaction is not
assumed to be idempotent: a configured secret can overlap the placeholder.

Delete `extend_redactor` and `SharedRedactor` after migrating callers. Remove
subscriber redactor access and presenter redaction of already-prepared
events. `McpSubscriber::with_redactor` callers instead create a root redacting subscriber.
This is an intentional Rust API migration, not a no-op compatibility shim
that would silently stop protecting older callers.

### 4.3 Governed fields and sinks

Provide explicit, exhaustively tested sanitizers for event payloads rather
than relying on each presenter to remember fields. Cover command display,
task failure/retry/skip text, generator lifecycle/action messages and targets,
prompt rejection text, script log messages and targets, notice messages and
recursive JSON fields, and diagnostic text/details/hints. Audit identifiers,
paths and other dynamic labels too: any user-derived text that can contain
an input value must be sanitized in its display projection. Preserve raw
identity internally for routing; do not change task identity by redacting a
map key. Stable codes, levels, counters and other protocol metadata retain
their meaning.

Presenters handle routing, quiet filtering, deduplication, bounds and
rendering only. Channel subscribers and custom/tracing subscribers receive
the same sanitized events. Queued events carry sanitized data, not redactor
references, so draining a queue later needs no live operation scope.

Existing explicit producer-side tracing of governed strings must use a
sanitized projection before `tracing::*` is called. `#[instrument]` must skip
raw inputs, redactors and scope internals; avoid secret-bearing `Debug`
implementations. Do not rely on a formatter-side filter: another tracing
layer could observe the original fields. Third-party telemetry and raw
process byte streams remain outside the contract, as before.

### 4.4 Preparation and incremental inputs

An immutable scope does not solve late discovery by itself. Establish a
preparation phase **before** any output that could include supplied values:

- Tasks: build the effective redactor from the resolved context before
  rendering commands, publishing task events, or formatting preparation
  failures that include args/env. A retry with changed values gets a new
  snapshot before publication.
- Generators/tools: seed a provisional scope from supplied values and known
  schema/policy before validation, template expansion or input collection.
  Incorporate defaults before a validation message can echo them. After
  resolution, construct the execution snapshot before actions start.
- Prompt collection: after receiving a value, classify it and build the next
  snapshot before emitting rejection feedback, validation errors or other
  messages containing that value. Previously collected secrets remain in
  that invocation's next snapshot. Password input stays non-echoing.

Pass the current snapshot explicitly through collection/validation helpers;
do not mutate a subscriber to make it visible. A producer discovering a
value late must withhold or sanitize the affected output before dispatch.
Do not buffer arbitrary raw events for retrospective redaction. Output
already emitted cannot be protected retroactively; tests pin the ordering
that prevents known supplied/defaulted/prompted secrets from reaching it.

### 4.5 Results and errors crossing scope boundaries

Publication includes returned output, not just callback events. Before a
child returns, sanitize its displayable success/failure projection with the
child's effective redactor. The parent must not need the child's redactor
to show that result later.

- Preserve raw execution values for computations and generated artifacts.
  Store sanitized projections for task summaries, generator action results,
  CLI rendering and MCP envelopes.
- Generator transactions need per-action provenance: today
  `run_in_transaction` infers the shared transaction's action list only after
  nested generators return. Keep raw transaction paths/data for execution,
  but record each pending action's sanitized display projection under its
  owning generator's effective scope before that scope ends. Carry that
  projection alongside the action through transaction merge/checkpoint/
  rollback, dropping it when the corresponding action is discarded. Final
  action inference must retain this prepared text (including paths), not
  reconstruct it from raw transaction data using only the root redactor.
  If inference needs to run later, sanitize all display-bearing source
  fields while the owner is alive and infer from that prepared metadata.
  `GeneratorRunResult.actions` and `GeneratorRunResponse.actions` consume
  these safe projections; neither retains child redactors or raw display
  fallbacks.
  Returned action paths are **display-only labels**, not filesystem
  references: a sanitized label can name a nonexistent or different file and
  must not be passed to filesystem APIs for inspection. Prepare normalized,
  workspace-relative labels before sanitization where possible. If
  relativization is impossible (for example, workspace `C:` and output `D:`
  on Windows, or incompatible UNC roots), prepare a normalized absolute
  display label and sanitize it under the owning scope instead. Both forms
  remain display-only; presenters must not clean,
  resolve or relativize the sanitized text as a path. Raw paths remain only
  in the execution transaction. Do not add a raw-path field or lookup API to
  bypass this protection.

    Preserve reporting identity separately from these labels. Within a run,
    the transaction assigns an opaque report key to each equivalence class
    used by the existing CLI summary: action category plus normalized path,
    or ordered `(from, to)` pair for rename/copy. Allocate keys from a run-local
    counter, not a hash/encoding of a path or secret; they have no meaning
    across runs and are not file-access handles. Repeated equivalent actions
    reuse a key, while distinct paths receive distinct keys even when their
    safe labels collide. Keep the raw equivalence map only inside the
    execution transaction, and carry the key with each safe projection through
    merge/checkpoint/rollback. Never reuse a discarded key for another identity
    within the run. Drop the map with the transaction. The Rust result/response
    carries this internal reporting metadata without serializing keys into MCP.
    `report_generator_output` uses ordered maps keyed by report key rather than
    sets of safe paths; it renders their prepared labels and counts their
    entries. Two distinct files may therefore appear as two identical safe
    labels with a count of two. No presenter needs raw paths to deduplicate.

- Preserve diagnostic code, audience, structured details and hints. Build
  and sanitize `DiagnosticReport` while the scope is alive. For plain errors,
  sanitize every displayed chain/context string, not just the top message.
  Carry a redacted error across the API boundary; do not erase a
  diagnostic into a string or let a fallback formatter expose its raw source.
- Internal error sources may remain typed for control flow, but user-facing
  and tracing error exits must use the redacted error. That copy
  contains neither raw sources nor a redactor. Audit all early `?` exits.
- Keep the existing tool success-value contract: the tool's own JSON result
  (`ToolRunResult.result`) remains untouched, as RFC 0023 specifies. It is
  user-requested data, not an omni diagnostic. Redact omni-owned sibling
  metadata, notices and failures consistently. Secret-bearing tool results
  are an explicit limitation; changing that contract is separate work.

This makes child-only failure text safe even when a parent later aggregates
it after the child scope has been dropped. Sanitized child projections are
not reprocessed using an unrelated parent's or sibling's local values.

### 4.6 Lifetime, cancellation and cross-operation masking

Scope clones may share their immutable `Arc` only with work belonging to that
operation. Join or cancel owned workers, drain/sanitize producer queues and
finish safe return projections before normal completion. Cancellation drops
the scope and aborts owned work through existing structured cleanup; detached
workers must not keep an operation alive indefinitely. A surviving child
holds inherited values only for its own remaining lifetime.

No subscriber, presenter buffer, tracing span, session history or global
registry stores the redactor. The final owning reference releases its
values, including on failure. This is a lifetime guarantee for redaction
state, not for all copies of the original inputs.

**Intentional cross-operation masking:** descendants mask inherited parent
secrets, and siblings mask values explicitly present in their common root.
This is conservative protection for shared data. A child-only secret is
never promoted upwards or sideways, even if another operation prints the
same literal text. Such output is redacted only when that operation also
declares or inherits the value. Shared output destinations do not establish
a trust relationship. Always-on URL-userinfo/credential rules remain
independent of concrete-value scope and are not weakened.

## 5. Test plan

Use distinct secret canaries and barriers/channels for deterministic
interleaving, not sleeps. Test both captured presenter output and data seen
by custom subscribers/tracing layers, so safety is not formatter-dependent.

| Case                                                                   | Required assertions                                                                                                                                                                                                                                                                                      |
| ---------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Concurrent tasks and independent tool/generator calls sharing one sink | Each hides its own secret; a literal matching only the other's secret remains unchanged, regardless of registration/emission order                                                                                                                                                                       |
| Nested generator and explicit nested tool scope                        | Child hides parent and child secrets; parent and sibling do not acquire child-only secrets; child failure remains safe when aggregated later                                                                                                                                                             |
| Nested generator transaction actions                                   | A child-only secret in an action path is absent from `GeneratorRunResponse.actions` after child completion, in dry-run and committed runs; real execution paths stay unchanged, and checkpoint/rollback keeps projections aligned with retained actions                                                  |
| Action label collisions and identity                                   | Two distinct paths with identical safe labels remain two reports/counts; repeated equivalent actions deduplicate; rename/copy pairs preserve identity; discarded keys are not reassigned on rollback; returned labels are never resolved as filesystem paths and reporting keys are absent from MCP JSON |
| Non-relativizable action paths                                         | Windows cross-drive and incompatible UNC-root output paths use owner-sanitized absolute display labels in CLI/MCP, including a child-only secret in the path; execution paths stay unchanged and presenters neither resolve nor re-sanitize labels                                                       |
| Subscriber reuse                                                       | Completed/failed/cancelled A does not affect B; presenter never retains A's redactor                                                                                                                                                                                                                     |
| Preparation                                                            | Supplied values, defaults and rejected prompted values are protected before execution begins, including early validation/expansion failures                                                                                                                                                              |
| Success/failure exits                                                  | Dry-run command, lifecycle/action/script events, notices, structured diagnostics and plain error chains are safe on both paths; tool success JSON stays unchanged                                                                                                                                        |
| All surfaces                                                           | CLI stream/progress/TUI buffers, MCP success metadata/error envelopes, channel/custom subscriber, trace stderr/file and independent custom tracing layer see safe projections                                                                                                                            |
| Lifetime                                                               | Weak-reference/drop instrumentation proves no redactor is retained by sinks, queued events, redacted results and errors or completed/cancelled workers                                                                                                                                                   |
| Publication contract                                                   | Notice mirror emitted once; lazy/demand/batch semantics preserved; no second redaction even when a secret overlaps the placeholder                                                                                                                                                                       |

Retain RFC 0022 policy/mode, short-value, nested-leaf and cache-key tests, and
RFC 0023 quiet filtering, notice deduplication/bounds and MCP transport tests.
Add a field-coverage test for each typed sanitizer and exercise new dynamic
fields through both CLI and MCP projection paths. Explicitly pin raw task
byte-stream passthrough and the tool-result exception rather than claiming
they are sanitized.

## 6. Compatibility and rollout

No config, CLI flag, placeholder, diagnostic code or MCP envelope shape changes.
The intended visible difference is removal of incidental cross-operation
masking and earlier protection of preparation output. Diagnostic structure,
notice ordering/filtering and existing byte-stream/tool-result contracts
remain intact.

Generator action paths deliberately change semantics from usable filesystem
paths to display-only labels. This is a client compatibility change despite
preserving the wire shape: API/MCP consumers must stop using returned action
paths to open generated files, even when a particular label has no redaction.
Document this in the API/MCP contract and release notes when implementation
lands. The Rust action result representation also gains internal report keys
and prepared labels; migrate CLI and API callers together. Exact execution
paths are not exposed as a compatibility fallback. Supporting private file
inspection through another interface would require a separate design.

1. Agree on this draft and the explicit tool-result/cross-operation limits.
2. Add the immutable redacting subscriber, event sanitizers and redacted result and error
   projections, with publication and isolation tests.
3. Migrate task preparation/execution, generator collection/nesting and tool
   validation/execution to the same scope contract. Audit early returns and
   direct tracing calls along with normal callbacks.
4. Switch CLI/MCP to redacted data only; remove all shared redactor APIs and
   redundant presenter redaction in the same migration. Do not leave a
   mixed path where a shared sink still unions descendant values.
5. Run the test matrix and workspace checks, document limits, then mark this
   RFC implemented with any deviations. Update RFC 0022/0023 ownership notes
   to reference this replacement when implementation lands.

Each intermediate implementation commit must preserve redaction coverage;
do not remove the old protection until its producer boundary is in place.

## 7. Alternatives and review questions

- **Keep the shared union:** simple and conservative against disclosure, but
  retains the lifetime/over-masking problems in #58.
- **Register/unregister scoped secrets in a shared set:** reference counting
  can bound retention but still masks concurrent siblings and makes delayed
  output depend on unregister timing.
- **Per-request subscribers only:** isolates requests but not parallel tasks
  or nested generators within a request, and prevents useful presenter reuse.
- **Task-local/global tracing redactors:** implicit propagation is fragile
  across spawned work and does not protect custom sinks before dispatch.
- **Attach redactors to events/errors:** handles delayed formatting but
  prolongs secret retention into output buffers and presenter lifetimes.

Review should confirm the explicit redacting-subscriber migration and redacted-error
boundary, and whether preserving raw tool success results remains the right
contract. The proposed decision is to preserve it, document the limitation,
and avoid expanding this scope/lifetime fix into a tool-data API change.

## 8. Implementation status and deviations

Implemented as designed:

- `omni_messages::RedactingSubscriber` (4.2): immutable per-operation redactor,
  `child` inheritance over one sink, sanitizing for notices, task and
  generator events and script logs before the `tracing` mirror and the sink.
  `NoticeSubscriber::publish_notice` is the producer entry point so the mirror
  only sees sanitized data. `SharedRedactor`, `redactor()`,
  `extend_redactor` and `McpSubscriber::with_redactor` are removed, and the
  CLI and MCP presenters no longer redact (4.2, 4.3).
- Tasks (one scope per task invocation, reused by retries), generators (one
  scope per invocation, nested scopes inherit the parent's effective
  redactor) and tools (one scope per call) all publish through it (4.1).
- `omni_diagnostic::redact_report` and `RedactedDiagnostic` (4.5): task
  resolution failures, generator run failures and tool failures are
  sanitized as they leave their scope and keep their code, audience, details,
  hints and context chain. An error with nothing to hide is returned as is.
- Tests: isolation under concurrent operations, reuse, nested inheritance,
  success and failure paths, the tracing mirror, overlap with the
  placeholder, and the redactor being released (`omni_messages`,
  `omni_generator`, `omni_task_executor`, `omni_diagnostic`,
  `omni_cli_core`, `omni_mcp_core`).

Deviations, with the reason:

- **A run's returned labels and failure use one union per run (4.5).** The
  design records each pending action's label under its owning generator's
  scope. That needs the owner threaded through the transaction overlay, which
  handlers reach only as a generic `sys`. Instead a `RunSecrets` collection,
  owned by the root run and dropped with it, gathers every generator
  invocation's secrets, and the run uses it to sanitize the action path
  labels and the failure it returns. Nothing leaks, and no other operation is
  affected, but inside one run a child-only secret can mask text in the
  parent's returned labels. Per-owner provenance is a follow-up.
- **No opaque report keys, no workspace-relative labels (4.5).** Returned
  action paths are the same paths with secrets replaced, so they are
  display-only and are not valid for filesystem access when a secret was
  present. Two distinct paths that sanitize to the same text collapse in the
  CLI summary count. The keys, the cross-drive label rule and the
  collision test stay in the design as the intended follow-up.
- **Input collection keeps its own redaction (4.4).** Validation and prompt
  messages continue to use the existing secret and `redact.inputs` handling
  in `omni_input_schema`/`omni_input_provider`; the generator's scope starts
  once its inputs are collected. Tool scope starts from the supplied values
  before validation, as designed.
- **`RedactingSubscriber::sink` is public** so a nested generator run can be
  configured over the same subscriber. Producer code emits through the
  redacting subscriber; the sink is for scope construction only.
- **Not tested:** cancellation of an in-flight operation and the
  `GeneratorRunResponse.actions` path through MCP are covered by the unit tests
  above but have no dedicated end-to-end test.
- **Unchanged limits:** raw task byte streams, the tool's own success JSON and
  third-party telemetry are not sanitized, as in sections 3 and 4.5.
