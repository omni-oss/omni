# R03.1 audit: command operand handling

Where a command string/argv element is handled, and where literal intent is lost.

| Stage | Location | Behavior | Literal intent lost? |
| --- | --- | --- | --- |
| Validate | `omni_command_config` `CommandConfig` deserialize; `omni_configurations` `TaskConfiguration` deserialize | Every string/element checked as Tera syntax | Yes: a literal containing `{{` fails validation or is treated as template |
| Resolve | `omni_command_config::resolve_command_with` | Tera (only if markers) -> env expand -> (Shell only) shlex split | Yes: Tera and env expansion run on every element |
| Resolve (generator) | `omni_generator` `run_custom_commons` | Tera applied earlier, then `resolve_command` on a `Shell` string (env expand + split) | Yes: rendered data is env-expanded and split |
| Display | `batch_executor::resolve_task_command_for_display`, `format_argv` | Same resolve with no env; shell-quotes the argv | No: argv is quoted, not reinterpreted |
| Hash | `omni_collector` `collector.rs` (`canonical()`), `omni_cache` hybrid | Raw `canonical()` text | Yes: nothing distinguishes interpretation mode |
| Serialize | `CommandConfig` serde/schemars | Untagged string or array | Yes: no representation for a literal |

Resolution: `CommandOperand` (`Literal` / `Expression` / `Interpolated`) carried by
`CommandConfig::Operands`. Config-authored strings remain `Shell`/`Argv` (expression
semantics, unchanged canonical text and hashes). `Operands` is not deserializable and is
excluded from the schema (wire form is R03.5, deferred).

Known gaps: generator path still builds a `Shell` string; `Interpolated` filters act on a
placeholder, so value-transforming filters (`upper`) do not affect bound data.
