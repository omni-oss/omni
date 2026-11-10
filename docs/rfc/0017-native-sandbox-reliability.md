# RFC 0017: Cross-platform capability security boundary

- **Status:** Draft
- **Owner:** TBD
- **Date:** 2026-10-27
- **Affected crates/files:** `omni_capabilities`, `omni_capability_enforcement`, `omni_capability_sys`, `bridge_rpc_runner`, generator execution, native sandbox tests, CI
- **Prior art referenced:** WebAssembly/WASI, Linux Landlock, Bubblewrap/Flatpak, Bazel, Windows AppContainer

## 1. Summary

Make capabilities a consistent security boundary across supported operating systems, rather than composing OS-dependent controls with bypassable JavaScript patches. The recommended direction is a restricted WebAssembly guest with explicitly brokered host operations; JavaScript would execute through an interpreter compiled into that guest. Compatibility with arbitrary Node/Bun/Deno programs and native executables is not promised by this mode. Native backends become optional defense in depth or explicitly weaker compatibility execution, not substitutes for missing security guarantees. This is a proposed architecture requiring a feasibility prototype, not an implemented or audited boundary.

## 2. Motivation / Problem

GitHub Actions reports a failed positive executable-grant assertion on Ubuntu and error 2 during a confined Windows launch. The Linux helper discards the error information needed to identify its cause. Windows support detection derives a SID without testing process creation. Passing on Fedora does not establish that runner mounts, loaders, ACLs, profiles, or runtime dependencies are equivalent.

The supplied logs do not establish either root cause. Separately, the Windows implementation shares a sandbox SID across launches, allowing concurrent children to reach the union of that SID's grants. This is an isolation defect, not merely a test-reliability issue.

The clarified requirement is stronger than fixing those defects: capability guarantees must not depend on the host OS, and every route to a governed resource must cross a non-bypassable enforcement boundary. Hardening the current native backends alone cannot meet that requirement.

## 3. Goals / Non-goals

### Goals

- Make launch failures distinguishable from expected policy denials.
- Verify both permitted and denied operations on supported CI hosts.
- Preserve fail-closed execution when a required guarantee cannot be established.
- Isolate mutually untrusted Windows launches from each other's grants.
- Keep the policy engine platform-neutral.
- Provide the same required capability contract on every supported target; refuse secure execution if that contract is unavailable.
- Remove ambient guest access to host files, sockets, environment, native libraries, and process creation.
- Make all guest authority explicit and validate requests outside the guest.

### Non-goals

- Claim complete filesystem, process, network, or kernel isolation from Landlock alone.
- Preserve arbitrary Node/Bun/Deno compatibility inside the proposed restricted execution mode.
- Treat ordinary native process execution as confined merely because the spawn request was authorized.
- Infer that any spawn error proves sandbox enforcement.
- Broaden system grants or disable host protections merely to make CI green.

## 4. Background: current mechanisms

- `crates/omni_capabilities/src/lib.rs`: pure policy evaluation and projection; no OS integration belongs here.
- `crates/omni_capability_enforcement/src/spawn.rs`: `OsSandboxSpec` describes subtree grants, executable names, TCP ports, and a `confine` marker.
- `src/platform.rs`: `NativeOsSandbox` reports coverage and lowers policy. Windows currently reports no policy filesystem floor, despite older module comments claiming otherwise. `install_os_sandbox` installs Unix `pre_exec` hooks; its empty-spec check currently ignores `confine`.
- `src/landlock_sandbox.rs`: `restrict` builds filesystem V3 rules and optional TCP-connect rules. Baseline grants include system executable/library trees and `/proc`, `/sys`, `/dev`. Read grants include execution. `NotEnforced` aborts launch; partial enforcement is accepted.
- `tests/landlock_spawn.rs`: the executable test copies `cat` into granted and denied directories. `ran_successfully` retries every failure and discards OS errors, exit status, and stderr. Thus an unrelated failure can also satisfy the negative assertion.
- `src/appcontainer_sandbox.rs`: `spawn` creates or derives `OmniOssSandbox`, adds ACLs, and launches with security capabilities. `grant_read_scope` uses that same identity. Guards revoke grants using a process-local registry keyed by path. This does not isolate concurrent policies, coordinate separate omni processes, or track permission masks separately.
- Profile recovery assumes SID derivation establishes registration readiness. A deterministic identity calculation is not a real confined-launch readiness check.
- `crates/bridge_rpc_runner/src/runner.rs`: runtime essentials, native spawn, and scoped import grants are the integration points. Changing identity requires threading it through spawn, scoped grants, and broker-channel permissions together.

Discovery, configuration merge, capability extraction, and graph construction need no immediate change. Execution setup and its tests are the affected pipeline stages.

Local verification on the supplied Fedora host: `cargo test -p omni_capability_enforcement --test landlock_spawn -- --nocapture` passed all seven tests. Windows was not executable on this host.

## 5. Proposed design

### Primary direction: a portable, broker-only guest

The user clarification supersedes native hardening as the primary architectural recommendation. Prototype a maintained WebAssembly engine such as Wasmtime on every supported CPU/OS target, hosting a JavaScript interpreter compiled to WebAssembly. The engine supplies guest memory/control-flow isolation; the host supplies only deliberately defined imports. A native embedded interpreter or V8 isolate alone is not an equivalent containment boundary against interpreter/native-memory vulnerabilities.

```text
Untrusted JavaScript and its dependencies
                  |
JavaScript interpreter inside a WebAssembly guest
                  |
Explicit, validated host imports (no ambient WASI authority)
                  |
Trusted broker: session identity + omni_capabilities policy
                  |
Filesystem / network / environment / approved tool services
```

- Expose no unrestricted WASI filesystem preopens, inherited environment, sockets, native addons, FFI, or native process-spawn API. Audit the interpreter's libc/WASI imports as well as JavaScript-visible APIs. Default-deny unknown imports.
- Reuse `omni_capabilities` evaluation and the existing filesystem/environment enforcement in `PolicyEnforcingSys`; add broker services for network and safe tool execution, which `BridgeBroker` currently does not mediate. Guest-side adapters provide ergonomics, never authorization.
- Bind effective policy and caller identity to the host-owned guest session. Do not trust a guest-supplied policy, session ID, or claimed parent. Derived sessions cannot widen authority.
- Serve module source through the broker. Define import-read authority explicitly; avoid scanning code in an unrestricted native runtime as part of secure execution. Runtime/interpreter assets are trusted boot inputs, not general host directory grants.
- Enforce filesystem authorization on the actual object reached. Lexical glob checks alone are insufficient for symlinks, junctions/reparse points, and rename races; host adapters must use handle-relative operations and reject unsupported safety conditions. Some OS-specific trusted implementation remains unavoidable, but the external guarantee must not weaken.
- Network access goes through a host service, not raw guest sockets. Authorize each redirect and actual connection destination, address DNS rebinding/private-address rules, and define proxy behavior. A permitted hostname must not silently grant arbitrary destination access.
- Give each guest bounded memory, execution budget/deadline, output, and outstanding host calls. Interrupting guest execution is insufficient unless blocked host I/O is also cancellable/bounded. Prefer a worker process for crash containment; it is not itself the portable security primitive.
- Keep native Landlock/AppContainer restrictions optional defense in depth. Failure of those optional controls must not remove any guarantee the portable mode claims. A missing required WebAssembly feature instead refuses execution; no silent native fallback.

### Native process capability is a separate authority decision

Authorizing `spawn("tool")` does not confine that tool's later filesystem/network behavior. An ordinary native child can bypass the guest boundary through its own syscalls. Therefore secure mode must initially deny arbitrary native children, support guest-contained WebAssembly tools, or expose narrowly designed trusted host tools whose argument/input behavior is part of the trusted computing base. Trusted tools must not expose shell, plugin-loading, or arbitrary script execution escape hatches. A per-process restricted native child with equivalent guarantees needs a separately managed isolation boundary; a sandboxed VM/remote worker is an option, not an automatic property of the broker.

If full npm/native-addon compatibility and arbitrary executables are mandatory, recommend a controlled VM execution environment instead. On Windows/macOS a managed Linux VM can unify guest enforcement, at the cost of provisioning and a guest filesystem model rather than transparent native-host execution. Containers are an environment option, not a uniform separate-kernel boundary on every host.

### Feasibility gate and migration

Before accepting the design, prototype real generators, TypeScript preprocessing, ESM imports, asynchronous broker calls, cancellation, and performance. Evaluate a maintained JS-to-Wasm interpreter rather than assuming Node itself can simply become a WASI guest. No Wasmtime or embedded-JS dependency is currently present in the workspace Cargo manifests searched during this review.

### Embedded JavaScript alternative

Embedding a JavaScript engine directly is a viable, simpler implementation of the broker-only API model. JavaScript itself has no filesystem, sockets, environment, or process API: those powers come from the embedding. A bare engine with only checked omni bindings can therefore prevent malicious JavaScript from bypassing capability checks through ordinary language operations, without WebAssembly. Embedding the full Node/Deno host environment would not provide this property unless all ambient resource APIs were removed or completely mediated.

The distinction is the engine-compromise threat model. A directly embedded engine and its host bindings execute with omni's process privileges. A memory-safety or engine-escape vulnerability can compromise that process; a JS interpreter inside a correctly configured WebAssembly guest adds a containment boundary against guest-interpreter compromise, though Wasm engine and host-call vulnerabilities remain trusted risks. A bare worker process improves crash/resource containment but is not a resource-access sandbox by itself.

Prototype a directly embedded, broker-only engine alongside the Wasm option before selecting dependencies. Boa is a Rust implementation worth evaluating; QuickJS and V8 embeddings are other candidates with different safety, compatibility, maintenance, and performance costs. No candidate is approved or audited by this proposal. Verify ESM resolution, promises/async host calls, termination, memory limits, TypeScript preprocessing, and generator compatibility. Neither direct embedding nor Wasm automatically preserves npm/native-addon compatibility. The architectural requirement is complete mediation with no ambient authority; Wasm is an additional isolation choice, not a prerequisite for every JavaScript-level capability boundary.

The policy vocabulary remains shared, but secure-mode representability must reflect the proposed process/network restrictions. Plan an explicit execution-mode selection and diagnostics; exact configuration/schema names remain open. Existing native runtimes must be labeled compatibility/best-effort for unsupported guarantees, not advertised as secure because their mediated routes pass policy checks. Selecting secure mode never falls back to them. The existing `direct` filesystem escape hatch cannot become unmediated host access in this mode; it must be rejected or implemented through equally checked guest APIs.

### Supporting work: establish evidence and repair native failures

1. Replace boolean-only executable observations with results retaining spawn errno, status, and stderr. Retry only an identified transient error such as `ETXTBSY`, with bounded attempts and retained diagnostics. Remove the unsupported suggestion that ordinary execution needs a settled page cache.
2. Run both copied binaries without confinement first. Their success proves the fixture is runnable on the selected filesystem. Distinguish fixture failure from a confinement regression; a required sandbox CI lane must not silently skip either.
3. Assert a specific execution-denial outcome for the denied fixture, not arbitrary non-success. Retain positive and negative filesystem behavior tests.
4. Record runner OS/image, kernel and Landlock ABI, executable/interpreter paths, temporary-directory mount options, and sanitized loader-environment information on failure. Never dump secrets or the full environment. Use syscall tracing in a diagnostic lane when necessary.
5. On Windows, attach stage context to profile creation, attribute construction, and child creation while preserving raw error codes. Record program, cwd, path lengths, SID, and profile-creation HRESULT. Compare the identical command/cwd/stdio without confinement. Check path handling and profile readiness rather than assuming error 2 means `cmd.exe` is absent.
6. Serialize profile creation/readiness within a process; do not treat recovery from unexpected HRESULTs as validated readiness merely because SID derivation succeeds. Cross-process initialization requires coordination or independent identities.

### Supporting work: harden the native boundary

- Introduce an owned Windows sandbox session with a unique identity per isolation boundary. Descendants intentionally share the session; independent launches do not. Scope ACL grants and broker permissions to that identity, and keep cleanup alive until the confined process tree is finished. Profile deletion and crash recovery require an explicit lifecycle design before implementation.
- For any shared grant bookkeeping, serialize grant/registration/revocation transitions and track identity, normalized path, access mask, and ownership. Currently `grant_path` runs before the registry lock, permitting a revoke between grant and registration. A path-only refcount also retains widened write permission until every reader exits.
- Keep Windows boot grants narrow. Do not imply that an ordinary AppContainer removes access already granted to application packages by the host.
- Move Landlock preparation and allocations out of the post-fork hook. Prefer a prepared ruleset installed with minimal child-side operations; evaluate a small launcher if the crate's API cannot support this safely. Allocations after a multithreaded fork are not justified by the current safety comment.
- Specify minimum enforced rights rather than assuming every `PartiallyEnforced` result only drops networking: filesystem V2/V3 rights can be unavailable too.
- Report limitations honestly: baseline executables remain runnable; deny exceptions below a granted subtree are not native restrictions; this implementation's network rules do not enforce hostnames or UDP. Broker/shim enforcement applies only to operations that cannot bypass those boundaries.

### CI contract

Use explicit OS-image labels for the required sandbox matrix, record the actual image/kernel versions, and keep a moving-image compatibility lane. Image labels alone do not freeze the kernel or image revision. Extend the existing `OMNI_REQUIRE_OS_SANDBOX` test convention consistently to Linux and live Windows unit tests. An unavailable backend must fail required lanes; local unsupported hosts may report a clear skip. Test parallel independent launches, not just serialized smoke tests.

## 6. Mandatory analysis

### 6.1 Existing mechanisms (reuse vs. extend vs. add)

Reuse the policy engine, `PolicyEnforcingSys`, and floor analysis. Add a portable guest execution backend rather than attempting to express host imports as `OsSandboxSpec` path grants. `BridgeServiceRunner` currently owns a native child and OS-specific ACL lifetime; reuse service routing where feasible, but do not force a Wasm instance into a fake native-process abstraction. Windows identity ownership still needs repair for compatibility execution.

### 6.2 Maintenance cost

Portable execution adds a WebAssembly engine, a maintained JavaScript interpreter guest, reproducible guest builds, host-import ABI versioning, async adapters, security updates, and compatibility documentation. It reduces OS-specific policy composition but does not eliminate OS-specific filesystem safety work. Native repairs additionally require Windows session/profile lifecycle management and Unix fork-safe setup. Explicit execution-mode configuration requires schema/documentation updates once its shape is decided.

### 6.3 Testability

Add diagnostic-result unit tests and live positive/negative probes. Exercise executable fixtures outside confinement, exact denial results, missing loaders/paths, long Windows paths, parallel first-use profiles, grant/revoke interleavings, read/write overlap, separate omni processes, child/grandchild inheritance, and grants surviving until the last descendant exits. Windows behavior requires Windows CI; Unix unit tests cannot substitute. Kernel ABI coverage requires controlled hosts, not only distro containers sharing the host kernel.

Run identical adversarial guest fixtures on Linux, Windows, and macOS: direct WASI calls, unknown imports, native/FFI attempts, malformed broker messages, session spoofing, symlink/reparse/rename escapes, module-loading escapes, redirect/DNS policy bypasses, resource exhaustion, and host-call cancellation. Test native launch refusal under secure mode. Fuzz the host ABI and path-operation boundary; passing tests is not a security audit or proof of engine correctness.

### 6.4 Integration points

Policy projection and merging stay unchanged. `NativeOsSandbox` coverage must match actual enforced rights. Prepare native resources after resolving runtime essentials and before child creation. Carry Windows session identity through `spawn`, `grant_read_scope`, broker SID access, and cleanup ownership in `bridge_rpc_runner`. Establish failure context before crossing the process boundary.

For portable execution, select mode before enforcement planning, resolve modules through authorized services, instantiate a guest with host-owned policy, and register only the allowed import surface. Connect existing filesystem/env services and new network/tool services before execution. No changes to discovery or graph construction are intended; configuration, validation, and execution selection must carry mode explicitly once specified.

### 6.5 Potential regressions

Unique profiles increase cold-start and cleanup cost. Early cleanup can break surviving grandchildren. Path normalization, inherited ACLs, pre-existing grants, and crash residue complicate ownership. Tighter rights can break runtime initialization. Test retries can hide real regressions; arbitrary failures can falsely validate security tests. No hash format change is proposed, but sandbox execution tests must not be satisfied by an artifact cached on another OS/kernel. Any later guarantee that isolation changes task-visible inputs requires a separate cache-key review.

Portable mode can break npm packages, Node builtins, native addons, subprocess use, and runtime-specific globals; startup and interpreter throughput need measurement. The engine, interpreter, and host services remain trusted code and need timely vulnerability fixes. **Before implementation, cache keys must distinguish execution mode, guest runtime/ABI version, effective policy, and behavior-affecting tool/runtime inputs wherever cached generator results depend on them.** Existing hash paths have not yet been audited; native outputs must not silently satisfy portable-mode execution or its security tests.

### 6.6 Pros and cons

| Option                                             | Pros                                                                                              | Cons                                                                                                   | Verdict                                                                             |
| -------------------------------------------------- | ------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------- |
| Broker-only WebAssembly guest                      | Consistent default-deny host authority across OSes; existing policy engine reusable               | JS ecosystem compatibility, interpreter performance, substantial host-ABI work                         | Recommended direction, subject to feasibility gate                                  |
| Bare embedded JS engine with broker-only bindings  | Same portable API-level capability semantics; simpler integration, no guest interpreter toolchain | Engine compromise reaches host-process authority; embedding and resource control need review           | Evaluate first if engine vulnerabilities are outside the required containment model |
| Harden Landlock/AppContainer                       | Reuses existing integration; unprivileged native execution                                        | Different guarantees and lifecycle costs per OS                                                        | Supporting work; insufficient for clarified requirement                             |
| Bubblewrap plus optional Landlock/seccomp on Linux | Mount, PID, IPC, and network isolation; established Flatpak primitive                             | External dependency; user namespaces/host policy may block it; still needs a carefully designed policy | Consider for a separately specified stronger Linux mode                             |
| Restricted token/Job Object alone on Windows       | Useful privilege/resource/process-tree controls                                                   | Does not supply equivalent path allow-list enforcement by itself                                       | Complement, not replacement                                                         |
| Containers or VMs                                  | Controlled runtime environment; VMs add a separate kernel boundary                                | Provisioning, startup, sharing, and cross-platform cost                                                | Appropriate for hostile workloads requiring stronger isolation                      |
| Skip failing tests or broaden baseline access      | Quick green CI                                                                                    | Conceals regressions or weakens confinement                                                            | Reject                                                                              |

### 6.7 Prior art and how omni improves on it

| Tool                                                                                                                                            | Their approach                                                           | What to adopt                                               | What to change for omni                                                                             |
| ----------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------ | ----------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| WebAssembly/WASI capability model                                                                                                               | Guest code accesses host resources through provided imports/handles      | No ambient authority and explicit host interfaces           | Do not grant broad preopens; enforce omni rules per operation and define safe process semantics     |
| [Landlock](https://docs.kernel.org/userspace-api/landlock.html)                                                                                 | Stackable, inherited restrictions bounded by ABI and other host controls | Explicit handled rights and inherited confinement           | Separate supported rights from required guarantees; preserve launch diagnostics                     |
| [Bubblewrap/Flatpak](https://github.com/containers/bubblewrap)                                                                                  | Namespace construction with a caller-defined policy                      | Small native isolation boundary and private filesystem view | Evaluate namespace availability; do not claim Bubblewrap alone supplies omni's capability semantics |
| [Bazel](https://bazel.build/docs/sandboxing)                                                                                                    | Per-action execution roots and platform sandbox strategies               | Required backend selection and debuggable setup             | Preserve confidentiality goals; Bazel-style read-only host access is not a secret-read boundary     |
| [AppContainer SID derivation](https://learn.microsoft.com/en-us/windows/win32/api/userenv/nf-userenv-deriveappcontainersidfromappcontainername) | Derives the identity associated with a profile name                      | Explicit identity ownership                                 | Validate creation/readiness independently and isolate mutually untrusted sessions                   |

## 7. Open questions

- What are the discarded Linux errno/status/stderr and exact runner image/mount layout?
- Which Windows launch stage returns error 2, with which program/cwd and creation HRESULT?
- Which supported OS/CPU targets must the selected WebAssembly engine and interpreter cover?
- Must the boundary contain a compromised JavaScript interpreter, or is a trusted engine with malicious guest JavaScript the accepted threat model?
- Is compatibility with arbitrary npm packages/native processes mandatory, or is a restricted generator API acceptable?
- Which existing generators must the feasibility prototype run, and what performance budget is acceptable?
- What configuration and cache integration should select/identify secure versus compatibility execution?
- Should Windows identities be per generator tree or per runtime invocation, and how should broker import grants share them?
- Which Landlock rights are mandatory under strict filesystem-floor mode?
- Are managed dependencies or VM provisioning acceptable for an optional stronger sandbox mode?

## 8. Decision log

| #   | Decision                                                           | Rationale                                                                                                                   |
| --- | ------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------- |
| 1   | Diagnose before replacing native backends                          | Supplied failures do not establish primitive unreliability or a root cause                                                  |
| 2   | Retain native mechanisms only as supporting/compatibility controls | User clarification requires a portable, non-bypassable boundary instead of OS-dependent guarantees                          |
| 3   | Treat shared Windows identity as a security issue                  | Per-process refcounts do not prevent cross-policy grant union                                                               |
| 4   | Keep this proposal Draft                                           | CI evidence and the intended threat model remain unresolved                                                                 |
| 5   | Recommend a broker-only WebAssembly guest feasibility prototype    | Prevent bypass by removing ambient authority, rather than patching each native runtime API                                  |
| 6   | Deny arbitrary native spawn in initial secure mode                 | An authorized spawn is not confinement of the spawned tool                                                                  |
| 7   | Evaluate bare embedded JavaScript before requiring Wasm            | Complete mediation is possible in an embedding with no ambient APIs; Wasm primarily adds interpreter-compromise containment |
