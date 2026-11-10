# R03: Literal command operands separate from command expressions

**Type:** refactor + audit · **Size:** L · **Depends on:** none · **Unblocks:** F06

## Problem

Command argv strings are decoded with Tera validation, flattened, then expanded for
environment syntax. Any value that arrives as a plain string, such as a script name or
project name containing `$VAR`, `${VAR}`, `$(...)` or `{{ }}`, can be reinterpreted.
Structured argv alone does not fix this: the literal must be tracked through validation,
resolution, display, hashing and serialization.

## Why it matters

Standalone: safe handling of untrusted or data-derived operands (e.g. generators, MCP
callers, future pack content). For plugins: manifest-derived script names are data, not
commands; unexpanded handling is a security requirement.

## RFC references

- [§6.3 "Literal command operands"](../../006.03-data-boundary-and-literal-values.md#literal-command-operands)
- [§7.5 C16](../../007.04-composition-golden-fixtures.md)
- [§18.3 command decoding row](../../018.00-readiness-and-open-decisions.md#183-implementation-gates-grounded-in-current-code)
- [§8.3 definition vs runtime context audit](../../008.02-execution-cache-semantic-identity.md#83-fine-grained-execution-cache-integration)

## Code starting points

- `crates/omni_command_config/src/command_config.rs` (argv decoding, Tera validation)
- `crates/omni_command_config/src/resolve.rs` (Tera flattened before env expansion)
- `crates/omni_task_context/src/default_provider.rs` (plain JSON data into context)
- `crates/omni_collector/src/collector.rs` (hashes raw command config)
- `crates/omni_task_executor` (execution, display)

## Slices

1. **Audit note** (doc in PR): every place a command string/argv element is validated,
   resolved, displayed, hashed, serialized. List where literal intent would be lost.
2. **Type design:** command operand as `Literal(String)` vs `Expression(String)` (or an
   equivalent source-aware lowering plan). Existing config strings stay `Expression`
   so authored behavior is unchanged.
3. **Plumb through** validation, resolution (single pass, no extra Tera/env rendering of
   literals), display and canonical hashing. A literal and an equal-text expression must
   hash differently when they execute differently.
4. **Mixed path prototype (C16):** inferred data interpolated inside an authored
   expression. Prove the chosen design protects it before freezing any API.
5. Wire/serde/schema representation for later native↔Wasm transport (can be an internal
   type only for now).

## Out of scope

- Plugin SDK bindings; script-name generation.

## Acceptance

- Test matrix: literal containing `$VAR`, `${VAR}`, `$(cmd)`, `{{ x }}`, backslashes,
  flag-looking and delimiter-looking values; expression/literal transitions.
- Literal never expanded; no process spawned for `$(...)` literal.
- Authored expressions behave exactly as before (existing tests green).
- Hash goldens differ by interpretation mode where execution differs.
- Display output shows the real argv without re-interpreting.
