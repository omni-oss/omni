# yaml-language-server: $schema=https://raw.githubusercontent.com/omni-oss/json-schemas/refs/heads/main/project.json
name: "@omni-oss/{{ prompts.package_name }}"

extends:
{% if prompts.package_type == 'lib' %}
  - "@workspace/omni/presets/ts-vite-lib.omni.yaml"
{% elif prompts.package_type == 'app' %}
  - "@workspace/omni/presets/ts-vite-app.omni.yaml"
{% elif prompts.package_type == 'script' %}
  - "@workspace/omni/presets/ts-vite-script.omni.yaml"
{% endif %}

tasks:
  test:unit:
    enabled: {{ prompts.unit_test }}

  test:integration:
    enabled: {{ prompts.integration_test }}

  test:
    enabled: true

  build:
    enabled: true

  publish:
    enabled: {{ prompts.published }}

meta:
  publish: {{ prompts.published }}
