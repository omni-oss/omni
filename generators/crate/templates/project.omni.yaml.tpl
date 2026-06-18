# yaml-language-server: $schema=https://raw.githubusercontent.com/omni-oss/json-schemas/refs/heads/main/project.json
name: {{ inputs.crate_name }}

{%- if inputs.crate_type == "bin" %}
extends:
  - "@workspace/omni/presets/rust-cli.omni.yaml"
{%- elif inputs.crate_type == "lib" %}
extends:
  - "@workspace/omni/presets/rust-lib.omni.yaml"
{%- endif %}

{%- if inputs.use_tracing %}
dependencies:
  - trace
{%- endif %}
