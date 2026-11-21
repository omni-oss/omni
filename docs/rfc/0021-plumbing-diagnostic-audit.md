# RFC 0021 follow-up: plumbing diagnostic audit

An error earns a code when the location is specific enough to tell a user or
operator what to do. A transparent wrapper over an existing code should
forward it rather than invent another one. Generic errors remain plain where
they combine unrelated failure modes whose recovery depends on the operation.

| Area | Disposition |
| --- | --- |
| Remote-cache credential store | Code: `remote_cache.keyring_unavailable`. Check whether the current account can reach the credential store. Native initialization failures try the existing file fallback; an unavailable fallback or missing store reports the same code rather than returning backend text or panicking during encrypted config access. |
| Remote-cache machine ID | Code: `remote_cache.machine_id_unavailable`. Check that the machine identifier is readable. Do not include the identifier or platform error text. |
| Other setup | Keep the existing codes for declined access, insufficient scopes, and config reads/writes. No generic setup code: credential, file, and environment failures have different recovery paths. Generic I/O, seed encoding/crypto errors, and variable failures remain plain when not already wrapped by a specific diagnostic. |
| Process and PTY | Leave plain: PTY allocation already warns and falls back rather than returning `CantOpenPty`. Missing piped stdin/stdout/stderr after process construction are internal handle invariants. PTY reader/writer errors share variants with missing-pipe invariants; code them only at a call site that can distinguish the operating-system failure and give a specific recovery step. Existing spawn failures keep `process.spawn_failed`. |
| Hashing, collection, and cache store | Leave plain pending narrower call-site diagnostics: `HasherError::Io` and collector I/O are also used for non-read operations; collector glob/ignore variants mix project patterns and lower-level walker errors. Cache store failures were reviewed at the call boundary only; this work does not change protected cache code or hash behavior. |
| Tool runtime and bridge | Leave plain: the bridge error types mix transport, protocol, runtime and internal lifecycle failures. Some carry remote messages or raw payload context. Do not promote their `Display` strings to stable summaries. Use a tool/generator call-site error when it has a known termination reason or a useful action, not a generic bridge code. Existing user-facing tool and generator codes remain unchanged. |
| Projections | Code: `projections.destination_conflict` for whole-run preflight conflicts; users can change competing destinations or remove redundant rules. Forward already-coded reports. Generic I/O, ledger JSON, and hash errors stay plain; they cover several different operations. |
| Remote sources, lockfile, and Git | Leave plain here: the wrappers combine fetch, local worktree, metadata and filesystem operations. An operation-specific follow-up should distinguish bad configuration from transient or local environment faults. In particular, don't embed the raw Git/URL error in a diagnostic that promises safe details. |
| Planner/hash invariants | `CantGetTaskHash` has no constructor call anywhere in the tree (verified by search), so it cannot be reached. `CacheInfoNotFound` and `TaskEnvVarsNotFound` are lookups for tasks taken from the loaded execution plan, whose cache info and environment are populated during project extraction. `get_task_env_vars` can return `None`, so unreachability from user configuration is **not proven**. They stay uncoded because no user action maps to them: a hit means the plan and extraction disagree, which is a bug to report, not a configuration to fix. Revisit if a supported configuration is found that triggers them. |

Encrypted setup with the file fallback saves a newly generated key before
writing the config. It stages and syncs the key file, then replaces the
backing file atomically without changing its format. On Windows the staged
file is created with the existing owner, DACL, and explicit mandatory
integrity label, protected against adding parent permissions. Setup fails
without writing secrets if it cannot verify the original integrity label,
including the absence of a label on an existing file.
A new store starts with owner and Local System access only. The file
contents are synced before replacement, but the parent
directory is not: the guarantee is a successful reopen, not survival of a
power loss. The runtime fallback does not write snapshots on shutdown.
If saving fails, setup discards the
unpublished key and reports `remote_cache.keyring_unavailable` without
changing an existing encrypted config. Cross-process read-modify-write
locking remains tracked in [#65](https://github.com/omni-oss/omni/issues/65);
atomic replacement protects against torn files, not competing writers.

The credential diagnostics carry the environment audience. The CLI maps it to
exit `1`; MCP maps it to `internal_error` with the stable `kind` in its data.
Tests exercise the native/fallback decision with injected outcomes, missing
store lookup, backend error conversion and the shared diagnostic renderer.
They do not start a real platform keyring or fail either protocol end to end.
The machine-ID test constructs the sanitized error and checks that config
loading forwards its code. The projection CLI regression checks exit `2`,
the new code, the existing conflict message, and that nothing was written.
