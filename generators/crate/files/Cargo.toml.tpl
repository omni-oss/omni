[package]
name = "js_runtime"
authors.workspace = true
edition.workspace = true
rust-version.workspace = true
repository.workspace = true

[lib]
path = "src/lib.rs"

{% if prompts['use-tracing'] %}
[features]
default = ["enable-tracing"]
enable-tracing = ["trace/enabled"]

[dependencies]
trace = { workspace = true }
tracing = { workspace = true }
{% endif %}
