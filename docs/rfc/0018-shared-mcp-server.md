# RFC 0018: Shared MCP server with request-local working directories

- **Status:** Implemented
- **Owner:** TBD
- **Date:** 2026-10-29
- **Base branch:** `main`
- **Affected crates:** `omni`, `omni_cli_core`, `omni_mcp_core`, `omni_context`, `omni_api`, integration tests

## 1. Summary

Add `omni mcp --shared` so an agent can configure Omni once globally and start
its MCP server from any directory, including directories outside an Omni
workspace. Each workspace-scoped tool call supplies an absolute `working_dir`.
Omni discovers the workspace from that directory and executes with a
request-local context, without changing process CWD or retaining a selected
workspace between calls.

Keep the existing workspace-bound MCP mode unchanged. Workspace selection is
an MCP adapter concern; do not add directory selectors to every `omni_api`
operation request.

## 2. Goals and non-goals

### Goals

- Allow global agent MCP configuration without a workspace-specific startup path.
- Start and list tools without discovering or loading a workspace in shared mode.
- Make each call self-contained: location never depends on a preceding call.
- Support nested invocation directories, nested workspaces, and multiple
  workspaces in one server process.
- Preserve the distinction between workspace root and invocation directory.
- Reuse existing operation parameter types and API handlers where possible.
- Isolate concurrent calls' directory, environment, configuration, and log state.
- Preserve existing workspace-bound CLI and MCP behavior.

### Non-goals

- A network daemon, new MCP transport, or cross-user hosting.
- A workspace-selection tool or session-level default directory.
- Requiring agents to locate workspace roots themselves.
- Per-request overrides for every startup option, such as environment profile.
- Filesystem sandboxing or a new workspace authorization policy.
- Eliminating caches, generator sessions, or other intentional persistent effects.

`--shared` names the multi-workspace behavior. `--stateless` is not used because
it can imply transport semantics or prohibit internal caching. The interaction
is stateless with respect to workspace selection, not side effects.

## 3. CLI contract

Existing invocations remain supported:

```sh
omni mcp
omni mcp --root-dir /home/user/repos/app
```

The new invocation requires no workspace at startup:

```sh
omni mcp --shared
```

`--shared` and `--root-dir` are mutually exclusive. Declare the conflict in
Clap rather than manually checking it after parsing:

```rust
#[arg(long, conflicts_with = "root_dir")]
pub shared: bool,
```

This also rejects `--shared` when `root_dir` is supplied through
`OMNI_ROOT_DIR`. Report the conflict; do not silently ignore that variable.

Existing environment profile, environment-file, inheritance, and cache options
remain server-wide initialization policy. Selecting a working directory does
not mutate that policy. Relative explicit cache overrides in shared mode resolve
against each request's effective working directory, matching invocation-relative
CLI semantics. Configuration-owned paths retain their existing origins.

## 4. Tool-call contract

Every currently exposed MCP tool is workspace-scoped. In shared mode each
requires a top-level string `working_dir`, including tools that currently take
no arguments, such as `workspace_info`, `generator_list`, `hash_workspace`, and
`tool_list`.

Example `task_run` arguments:

```json
{
  "working_dir": "/home/user/repos/app/packages/frontend",
  "tasks": ["build"]
}
```

### 4.1 Directory validation and discovery

- Require a nonempty, platform-native absolute path to an existing directory.
- Reject missing, null, relative, nonexistent, and non-directory values.
- Do not expand `~`, environment variables, or shell expressions.
- Canonicalize through the filesystem before discovering the workspace; symlink
  aliases consequently use their physical directory's ancestor chain.
- Walk upward from the canonical working directory using existing workspace
  marker extensions and discovery precedence. Select the nearest workspace.
- Report a call-local error if no workspace is found or its configuration is
  invalid. Do not fall back to startup CWD, another workspace, or a previous call.
- No per-call `root_dir` selector is introduced in this version.

Keep unknown-tool errors independent of workspace discovery. Validate the tool
name and request shape before loading a workspace or executing an operation.

### 4.2 Working-directory semantics

The discovered root determines workspace configuration, project discovery, and
workspace-owned storage. The supplied directory is the effective invocation
directory for operations that already have invocation-relative semantics.

In particular:

- Relative generator output paths resolve from the effective working directory.
- Workspace environment queries start at the effective working directory;
  project-specific environment loading retains its explicit project directory.
- Relative operation paths that currently use process CWD use the effective
  working directory instead.
- Project filters, configuration-relative paths, and workspace-relative paths
  retain their existing meanings; not every path becomes invocation-relative.
- Child commands receive explicit execution directories and request-local
  environment maps. Do not mutate process environment variables.

Never call `std::env::set_current_dir` for shared requests, even under a mutex.
Other asynchronous work in the process must not observe temporary directory
changes.

### 4.3 `tool_run` naming collision

Bound-mode `ToolRunParams` already exposes optional `working_dir`, defaulting to
the workspace root. Preserve that schema and behavior in bound mode.

Shared mode exposes one required `working_dir`: it selects the workspace and is
also the workspace tool's execution directory. Pass its canonical absolute value
as `ToolWorkingDir::Path` to the API. Do not let the API's omitted-directory
default change execution back to the workspace root.

Use a shared-mode operation payload containing only `name` and `args` underneath
the common wrapper, rather than flattening the existing `ToolRunParams` and
creating two fields with the same name. Reuse the conversion/execution logic.
No separate execution-directory override is added initially.

## 5. Architecture

### 5.1 Startup and server state

The current MCP branch in `cli/omni/src/lib.rs` constructs a workspace context
before calling `commands::mcp::run`. Branch on shared mode before that context
construction. Keep process-level setup and shutdown owned by the CLI, not by
each request.

Represent server construction explicitly as bound or shared:

- Bound state retains the existing context and configuration fingerprint reload.
- Shared state stores the system implementation, `WorkspaceInitConfig`, and
  tracing policy, but no selected workspace or workspace-dependent context.

For the initial implementation, construct a fresh context for every shared
call. This naturally observes configuration changes and avoids an unbounded
workspace-context cache. A future bounded cache is permitted only if it preserves
request-local working directories, freshness, and isolation.

Do not hold a server-state mutex across asynchronous operation execution.

### 5.2 Explicit directory support in `omni_context`

Add a root-discovery entry point accepting a starting directory, for example
`get_root_dir_from(sys, start_dir)`. Keep `get_root_dir(sys)` as the
process-CWD-based compatibility entry point and delegate discovery to the common
implementation.

Add an optional effective-working-directory override to context initialization.
Preserve existing constructors' defaults. With an override,
`Context::current_dir()` returns that directory; without it, retain the current
system-CWD behavior. Preserve the override through cloning, loading, and reload.

The override must be available during construction, not assigned afterward:
cache override resolution can already consult process CWD during initialization.
Do not change the process CWD to make existing constructors work.

Audit direct `sys.env_current_dir()` consumers as well as `Context::current_dir()`.
In particular, `EnvLoader` currently reads system CWD independently. Give it the
effective directory as its fallback while retaining explicit `start_dir`
precedence. Context-based environment queries must not accidentally read the
server launch directory.

### 5.3 MCP parameter reuse and dispatch

`OmniApi` already accepts a prepared context through `new_with_sys`, and MCP
already creates fresh API instances per call. Keep API operation requests
workspace-agnostic.

Use an MCP-only wrapper, with this illustrative shape:

```rust
#[derive(Deserialize, JsonSchema)]
struct WorkspaceParams<P> {
    /// Absolute invocation directory; Omni discovers its workspace root.
    working_dir: PathBuf,

    #[serde(flatten)]
    params: P,
}
```

For argumentless operations, use a directory-only object or a tested empty
object payload, not a unit/null schema. Handle `tool_run` as described above.

Resolve location once per call and pass a request-local context or API factory
to tool adapters. Both normal APIs and subscriber-backed APIs must use the same
resolved context. Do not temporarily replace the server's bound context or use
task-local globals to select a workspace.

### 5.4 Schema publication

Publish schemas according to the immutable server mode:

- Bound mode retains existing parameter schemas and tool names.
- Shared mode adds required top-level `working_dir` to workspace-scoped schemas.
- Preserve operation defaults, required fields, annotations, and descriptions.
- Describe absolute-path requirements and automatic workspace discovery in the
  shared schemas and server instructions.
- Future workspace-independent tools do not require a working directory.

Use typed schema generation where possible. Verify Serde and Schemars agree on
flattened fields; do not rely on an unchecked JSON-schema mutation that differs
from runtime deserialization.

## 6. Errors, logging, and concurrency

Malformed parameters, missing directories, and invalid directory forms return
actionable invalid-parameter errors identifying `working_dir`. Workspace
discovery and configuration errors identify the requested directory and retain
useful underlying diagnostics. None of these failures terminate the server or
poison subsequent requests.

Keep stdout reserved for MCP protocol output from startup onward. Shared startup
must not require a writable Omni repository or create a workspace-local default
trace file in the arbitrary launch directory. Use stderr by default in shared
mode; explicit file-tracing options remain supported with a stable startup-based
path. Capture operation logs per request, preserving existing response options.

Concurrent calls must have separate effective directories, environments,
subscribers, and loaded metadata. Existing mutation coordination, including
generator serialization, must still coordinate by workspace across fresh
contexts. This feature does not promise arbitrary concurrent writes are safe.

Shared mode can address any locally accessible workspace under the server
process's existing privileges. It is not a sandbox or a safe unauthenticated
remote service. Existing operation capability policies remain in force.

## 7. Implementation scope

1. Add explicit-start discovery and request-local working-directory support in
   `omni_context`, including environment and cache-resolution consumers.
2. Add the shared CLI flag, Clap conflict, context-free startup branch, and
   appropriate tracing defaults.
3. Add shared server construction, MCP wrappers, mode-specific schemas, and
   request-local dispatch without changing `OmniApi` operation request types.
4. Adapt `tool_run` without duplicate flattened fields and preserve all bound
   schemas and behavior.
5. Document global agent configuration using `omni mcp --shared` and per-call
   `working_dir`.

## 8. Acceptance tests

- Shared startup, MCP initialization, and tool listing succeed from a directory
  with no Omni workspace; no workspace-local trace directory is created there.
- Clap rejects `--shared --root-dir ...` and `--shared` with `OMNI_ROOT_DIR` set.
  Environment-based parser tests must avoid unsafe process-global mutation.
- Bound-mode schemas and behavior remain unchanged.
- Every current shared tool requires exactly one string `working_dir`; existing
  operation required fields and defaults survive schema flattening.
- Formerly argumentless tools accept directory-only objects.
- Shared `tool_run` has no duplicate directory field and executes in the supplied
  subdirectory, not the workspace root.
- Missing, null, relative, empty, nonexistent, and file paths produce call-local
  errors; a subsequent valid call succeeds.
- Discovery selects the nearest workspace from a nested directory, supports
  existing marker extensions, and follows the documented symlink semantics.
- Generator output, environment queries, tool execution, and relative explicit
  cache overrides use the effective directory as specified.
- Alternating calls between workspaces and between directories within one
  workspace do not leak location, environment, subscribers, or metadata.
- Concurrent calls to different directories do not change process CWD or process
  environment and retain independent logs.
- Workspace/project configuration edits are observed on the next shared call.
- Existing generator coordination still serializes conflicting operations in
  the same workspace despite per-call context construction.
- Workspace initialization errors and unknown tool calls do not kill the server.
