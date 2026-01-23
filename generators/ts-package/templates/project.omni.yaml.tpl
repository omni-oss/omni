# yaml-language-server: $schema=https://raw.githubusercontent.com/omni-oss/omni/refs/heads/json-schemas/project-latest.json
name: "@omni-oss/{{ prompts.package_name }}"

extends:
  - "@workspace/omni/presets/ts-lib.omni.yaml"

tasks:
  build:
    enabled: {{ prompts.published }}
    command: bun x vite build

  test:unit:
    command: bun x vitest --config ./vitest.config.unit.ts run

{% if prompts.integration_test %}
  test:integration:
    enabled: {{ prompts.integration_test }}
    command: bun x vitest --config ./vitest.config.integration.ts run
    dependencies:
      - test:unit
{% endif %}

  test:
    dependencies:
      - test:unit
      {% if prompts.integration_test %}
      - test:integration
      {% endif %}
