# yaml-language-server: $schema=https://raw.githubusercontent.com/omni-oss/json-schemas/refs/heads/main/project.json
name: {{ prompts.crate_name }}

{%- if prompts.crate_type == "bin" %}
extends:
  - "@workspace/omni/presets/rust-cli.omni.yaml"
{%- elif prompts.crate_type == "lib" %}
extends:
  - "@workspace/omni/presets/rust-lib.omni.yaml"
{%- endif %}

{%- if prompts.use_tracing %}
dependencies:
  - trace
{%- endif %}
