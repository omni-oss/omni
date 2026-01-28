# yaml-language-server: $schema=https://raw.githubusercontent.com/omni-oss/omni/refs/heads/json-schemas/project-latest.json
name: "@omni-oss/{{ prompts.package_name }}"

extends:
  - "@workspace/omni/presets/ts-vite-lib.omni.yaml"

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
    enabled: {{ prompts.publish }}

meta:
  publish: {{ prompts.publish }}
