[package]
name = "{{ inputs.crate_name }}"
rust-version.workspace = true
edition.workspace = true
{%- if inputs.version == 'workspace' %}
version.workspace = true
{%- elif inputs.version == 'custom' %}
version = "{{ inputs.crate_version }}"
{%- endif %}
authors.workspace = true
repository.workspace = true

{%- if inputs.crate_type == 'bin' %}
[[bin]]
name = "{{ inputs.crate_name }}"
path = "src/main.rs"
{%- endif %}

[lib]
name = "{{ inputs.crate_name }}"
path = "src/lib.rs"

{%- if inputs.use_tracing %}
[features]
default = ["enable-tracing"]
enable-tracing = ["dep:tracing", "trace/enabled"]
{%- endif %}

[dependencies]
eyre = { workspace = true }
thiserror = { workspace = true }
strum = { workspace = true }
{%- if inputs.use_tracing %}
trace = { workspace = true }
tracing = { workspace = true, optional = true }
log = { workspace = true }
{%- endif %}
bon = { workspace = true }
derive-new = { workspace = true }

{%- if inputs.use_tracing %}
[dev-dependencies]
test-log = { workspace = true }
rstest = { workspace = true }
rstest_reuse = { workspace = true }
{%- endif %}
