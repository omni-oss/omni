# Diagnostic code prefixes

The prefix is the part before the first dot in an error or notice code,
such as `project` in `project.extends_cycle`. **One crate owns each prefix;
a crate can own more than one.** The registry rejects duplicate ownership
and codes submitted by a crate that does not own their prefix.

- Reuse a prefix your crate already owns when the new code fits that area.
- To surface a diagnostic owned by another crate, forward or delegate to the
  existing diagnostic rather than registering its code again.
- For a distinct area without a suitable prefix in your crate, pick an unused
  lowercase name, declare it with `omni_codes::prefixes!`, and add it to the
  table below. Check both error and notice prefixes.
- Keep existing spellings: changing a prefix changes the public codes that
  scripts and clients match on. Do not rename a prefix merely to make
  related crates look consistent.

## Production allocation

The declarations in Rust are the source of truth. The CLI registry test
checks that each production registration linked into the CLI has the owner
recorded here. Test-only prefixes are not listed.

| Prefix | Owning crate |
| --- | --- |
| `cache` | `omni_task_executor` |
| `capabilities` | `bridge_rpc_runner` |
| `capability_policy` | `omni_capabilities` |
| `code` | `omni_mcp_core` |
| `composition` | `omni_configurations` |
| `command` | `omni_command_config` |
| `config` | `omni_configurations` |
| `config_discovery` | `omni_configuration_discovery` |
| `discovery` | `omni_discovery` |
| `enforcement` | `omni_capability_enforcement` |
| `env` | `omni_cli_core` |
| `env_file` | `env_loader` |
| `env_parse` | `env` |
| `executor` | `omni_task_executor` |
| `expression` | `omni_expressions` |
| `file_data` | `omni_file_data_serde` |
| `generator` | `omni_generator` |
| `ignore` | `omni_ignore_core` |
| `input` | `omni_input_schema` |
| `input_provider` | `omni_input_provider` |
| `input_view` | `omni_input_view` |
| `meta` | `omni_execution_plan` |
| `meta_source` | `omni_meta` |
| `notices` | `omni_mcp_core` |
| `plan` | `omni_execution_plan` |
| `process` | `omni_process` |
| `project` | `omni_core` |
| `projections` | `omni_projections` |
| `redact` | `omni_task_executor` |
| `remote_cache` | `omni_setup` |
| `runtime` | `bridge_rpc_runner` |
| `sandbox` | `bridge_rpc_runner` |
| `scm` | `omni_scm` |
| `task` | `omni_core` |
| `task_alias` | `omni_configurations` |
| `task_extension` | `omni_configurations` |
| `tool` | `omni_tool` |
| `workspace` | `omni_context` |
