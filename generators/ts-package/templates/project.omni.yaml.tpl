# yaml-language-server: $schema=https://raw.githubusercontent.com/omni-oss/omni/refs/heads/json-schemas/project-latest.json
name: "@omni-oss/{{ prompts.package_name }}"

extends:
  - "@workspace/omni/presets/ts-vite-lib.omni.yaml"

tasks:
  build:
    enabled: {{ prompts.published }}

  test:unit:
    enabled: {{ prompts.unit_test }}

  test:integration:
    enabled: {{ prompts.integration_test }}

  test:
    dependencies:
      - test:unit
      - test:integration
