# RFC 0025: Derive Notice on enums

- **Status:** Implemented <!-- Draft | Accepted | Implemented | Superseded -->
- **Owner:** TBD
- **Date:** 2026-11-08
- **Base branch:** `main`
- **Affected crates/files:** `omni_notice_derive`, `omni_derive_support`, `omni_notice` (docs and tests), `omni_codes` (trait docs); optionally `omni_diagnostic_derive` (shared emission helper adoption)
- **Follows:** RFC 0023 (notices and the canonical code registry)

## 1. Summary

Allow `#[derive(Notice)]` on enums. Each variant describes an independent
notice: its own code, fixed level, message, docs and structured fields. The
enum groups related notices in Rust without grouping their identities in the
code inventory.

Keep the existing struct API and runtime traits unchanged. Generate one
`CodeRegistration` for the enum, containing one `CodeInfo` per variant, just
as diagnostic enums contribute their own codes today.

This RFC was the documentation-only first step. Implementation and tests
follow on `feat/notice-derive-on-enum` in the same feature PR.

## 2. Motivation and scope

`omni_notice_derive` currently rejects enums and emits direct `self.field`
accesses, one metadata entry and one message for a struct. This is a derive
limitation, not a restriction in `Notice`, `Code` or `omni_codes`.

Related non-fatal conditions can naturally form an enum, much as related
fatal conditions form a diagnostic enum. Requiring a separate struct for
each notice adds names without providing a runtime or inventory benefit.

**Goals**

- Support unit and named-field variants with the same rules as structs.
- Preserve each variant as a separate, discoverable code inventory entry.
- Preserve all existing struct behavior and both registry backends.
- Reuse small, genuinely shared emission helpers where useful.

**Non-goals**

- Tuple structs or tuple variants: structured notice fields still need names.
- Notice forwarding, generic notices, or opting out of registration.
- Enum-level defaults, code prefixes inferred from Rust names, or inherited
  metadata.
- Changes to notice delivery, serialization envelopes, CLI/MCP presenters,
  diagnostic semantics, or the registry schema.
- Migrating existing notice call sites merely to demonstrate the feature.

## 3. Public API

```rust
use omni_notice::Notice;

/// Non-fatal cache conditions.
#[derive(Notice)]
enum CacheNotice {
    /// Could not write a cache entry. The task result is unaffected.
    ///
    /// The result remains available for this run but was not cached.
    #[notice(
        code = "cache.write_skipped",
        level = error,
        stable,
        message = "could not write cache entry for '{entry}': {reason}"
    )]
    WriteSkipped {
        entry: String,
        #[notice(skip)]
        reason: String,
    },

    /// Cache verification is disabled.
    #[notice(
        code = "cache.verification_disabled",
        level = warn,
        essential,
        message = "cache verification is disabled for this run"
    )]
    VerificationDisabled,
}
```

Emission remains
`omni_messages::notice!(sub, CacheNotice::WriteSkipped { ... })`.
No changes are needed to `notice!` or `diagnostic_notice!`: they consume a
`Notice`, regardless of whether the value is a struct or enum variant.

### 3.1 Attribute placement and metadata

For a struct, attributes and docs remain on the struct. For an enum:

- Every variant requires its own `code`, `level`, `message` and nonempty doc
  summary. Enum docs describe the Rust grouping and are not a fallback.
- `essential`, `stable`, `deprecated`, repeated `related` and `docs` are
  supported on each variant, with their existing meanings and defaults.
  `docs = <expr>` overrides the explanation, not the required summary.
- Reject any `#[notice(...)]` attribute on the enum itself; metadata belongs
  on a variant. No merging or inheritance rules are introduced.
- Named fields accept only `#[notice(skip)]`. A skipped field is available
  to the message but omitted from structured fields, as on structs.
- A level remains fixed **per code**, not necessarily per Rust enum. Variants
  in the same enum may have different levels.

### 3.2 Validation

Apply the current struct rules independently to every variant:

- Codes use dotted lowercase segments with at least two segments.
- Levels are `info`, `warn` or `error`; message placeholders use named fields
  and the existing formatting behavior.
- An `essential` variant has no fields, including skipped fields, and no
  message placeholders. Literal escaped braces remain allowed. A named
  variant with zero fields is valid under the same rule as an empty struct.
- Non-skipped fields must serialize; fields used by a message must satisfy
  its formatting requirements. The enum itself need not derive `Serialize`.
- Reject tuple variants and unions. Continue rejecting generic notice types
  and notice types implementing `std::error::Error`; the latter check applies
  to the entire enum, just as it applies to a struct.

Reject duplicate codes within one enum at derive time, pointing to the
duplicate variant's code. Cross-type and cross-crate duplicates, prefix
ownership and related-code checks remain the registry validator's job.
Prefer errors at the offending attribute, field or variant over errors on
the enclosing enum.

An empty, non-generic enum is supported: its code list is empty and methods
use exhaustive `match *self {}` expressions. It adds no inventory entries.

## 4. Generated behavior and registration

Normalize each struct or enum variant into an internal notice case containing
its pattern, field bindings, message and validated metadata. A struct is a
single case; avoid keeping separate struct and enum generation pipelines.

Generate the same implementations as today:

- `Display`: match the active case and format its message, passing only
  fields referenced by the template.
- `Notice::fields()`: match the active case and build a `BTreeMap` from its
  non-skipped fields. Do not add an enum tag or a variant-name field.
- `Code::info()`: return the active case's static `CodeInfo`.
- `Code::CODES`: contain every case's metadata in declaration order.
- The sealed marker and existing check that the type is not an `Error`.

Bindings must be hygienic: use generated internal identifiers for pattern
bindings and map message argument names explicitly, so user fields cannot
collide with formatter, map or metadata identifiers. Use anonymous const
scopes for private metadata statics where appropriate; different derives in
the same module must not collide.

Submit **one** `omni_codes::CodeRegistration` per derived type, with
`codes: <Type as Code>::CODES`, the existing crate name and empty prefixes.
Keep `Type::__CODE_REGISTRATION` with its existing shape for the
`explicit-registry` backend. Consumers install that registration once, not
one registration per variant. Existing crate-level prefix declarations remain
necessary and unchanged.

Because `omni_codes::all()` flattens registrations' code slices, every enum
variant is a separate entry for lookup, validation, `omni code list`,
`omni code explain` and their MCP equivalents. All variants are registered
even if no value of that variant is constructed. No registry or presenter
changes are necessary.

## 5. Refactoring `omni_derive_support`

Keep the shared crate a collection of small parsing/emission utilities, not
a common framework for two different derives.

1. Continue using `valid_code`, `doc_parts`, `has_placeholder`,
   `placeholder_names`, `CodeInfoTokens` and `code_info` unchanged unless a
   focused test exposes a necessary fix. Enum support alone does not require
   a new format-string parser.
2. Extract a small `code_registration` expression emitter accepting the
   caller's `omni_codes` path and code-slice expression. It emits the common
   `CodeRegistration { crate_name: env!("CARGO_PKG_NAME"), codes, prefixes: &[] }`
   shape. Reuse it for inventory submission and `__CODE_REGISTRATION` so
   enum and struct registrations cannot drift. Unit-test its emitted shape.
3. The diagnostic derive may adopt that expression helper for its
   `omni_codes` submission in a separate, behavior-preserving change. Do not
   change its distinct `omni_diagnostic::Registration`, forwarding rules or
   `no_register` behavior.
4. Keep notice attribute parsing, notice-case normalization, field skipping,
   essential checks and `Display` generation in `omni_notice_derive`.
   Diagnostic fields and forwarding have different semantics; a shared
   case model would introduce more policy than it removes.

Only extract additional helpers (for example duplicate-code detection or
named-field patterns) if implementation demonstrates identical logic needed
by both derives. Such extractions must retain diagnostic behavior and be
covered by its existing tests. They are not prerequisites for enum support.

## 6. Compatibility and alternatives

This is additive: existing structs retain their attributes, formatting,
field JSON, code metadata and registration constant. `Notice`, `Code` and
`CodeRegistration` do not change. Update their docs where they currently
describe notice metadata or fields as struct-only, and document this RFC as
an extension of RFC 0023 rather than rewriting that RFC's historical design.

Alternatives:

- **One registration per variant:** valid at the registry level, but adds
  submissions and complicates explicit registration without changing entries.
- **Enum defaults:** save repetition but introduce precedence rules. Explicit
  per-variant metadata keeps each code's contract self-contained.
- **Tuple variants:** require a new naming/key convention; defer rather than
  expanding the existing named-field API.
- **One shared derive engine:** diagnostics have forwarding, audience and
  callback behavior that notices do not. Share emission primitives instead.

## 7. Implementation and validation plan

1. Land this RFC as the initial docs commit and open the feature PR in draft.
2. Add the shared registration-expression helper and its unit tests.
3. Refactor the notice derive into parsing, case validation and emission;
   add enums while retaining struct behavior and registration compatibility.
4. Update notice examples and trait documentation. Keep implementation
   commits on the same branch and update the draft PR as work progresses.

Tests must cover:

- An enum mixing unit and named-field variants, all three levels, distinct
  summaries/explanations, `docs` overrides, stability/deprecation and related
  metadata; escaped braces and field formatting.
- Skipped message-only fields, fields unused by a message, and field names
  colliding with generated implementation identifiers.
- `Code::CODES` declaration order, correct active `info()`, and separate
  inventory lookup/catalog entries for every variant.
- Default inventory registration and explicit installation using the single
  `__CODE_REGISTRATION`, including registry validation.
- Empty enums, zero-field named variants, and multiple derives in one module.
- Compile failures for missing per-variant metadata/docs, bad code/level,
  duplicate codes, enum-level attributes, unsupported field attributes,
  tuple variants, unions, generics, invalid essential variants and an enum
  implementing `Error`.
- Existing struct runtime and UI tests unchanged in behavior; diagnostic
  tests if its emission adopts a shared helper.

Run the targeted Cargo test suites for `omni_derive_support`, `omni_notice`,
`omni_codes` and `omni_messages`, plus `omni_diagnostic` when affected; run
the explicit-registry tests separately with that feature enabled. No runtime
performance change is expected beyond a match on the enum discriminant.

## 8. Implementation outcome

The choices above were confirmed: no tuple variants or enum-level defaults,
empty enums are supported, and the shared refactor is limited to the
registration-expression helper. `omni_diagnostic_derive` remains unchanged.

The implementation uses the same case emission for structs and enum variants,
with aliased field bindings and metadata statics in an anonymous const scope.
Runtime, UI and documentation tests cover the public API, both registry
backends and both notice emission macros. Existing struct UI snapshots remain
unchanged.
