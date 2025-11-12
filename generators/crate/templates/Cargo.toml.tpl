[package]
name = "{{ prompts.crate_name }}"
rust-version.workspace = true
edition.workspace = true
version.workspace = true
authors.workspace = true
repository.workspace = true

{%- if prompts.crate_type == 'bin' %}
[[bin]]
name = "{{ prompts.crate_name }}"
path = "src/main.rs"
{%- endif %}

[lib]
name = "{{ prompts.crate_name }}"
path = "src/lib.rs"

{%- if prompts.use_tracing %}
[features]
default = ["enable-tracing"]
enable-tracing = ["dep:tracing", "trace/enabled"]
{%- endif %}

[dependencies]
eyre = { workspace = true }
thiserror = { workspace = true }
strum = { workspace = true }
{%- if prompts.use_tracing %}
trace = { workspace = true }
tracing = { workspace = true, optional = true }
{%- endif %}
derive_builder = { workspace = true }
derive-new = { workspace = true }
