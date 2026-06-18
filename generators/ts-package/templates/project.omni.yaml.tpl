# yaml-language-server: $schema=https://raw.githubusercontent.com/omni-oss/json-schemas/refs/heads/main/project.json
name: "@omni-oss/{{ inputs.package_name }}"

extends:
{% if inputs.package_type == 'lib' %}
  - "@workspace/omni/presets/ts-vite-lib.omni.yaml"
{% elif inputs.package_type == 'app' %}
  - "@workspace/omni/presets/ts-vite-app.omni.yaml"
{% elif inputs.package_type == 'script' %}
  - "@workspace/omni/presets/ts-vite-script.omni.yaml"
{% elif inputs.package_type == 'e2e-tests' or inputs.package_type == 'service-tests' %}
  - "@workspace/omni/presets/ts-vite-test.omni.yaml"
{% endif %}

dependencies:
  append:
    - "@omni-oss/tsconfig"
    - "@omni-oss/vite-config"
    - "@omni-oss/vitest-config"

tasks:
{% if inputs.package_type == 'lib' or inputs.package_type == 'script' or inputs.package_type == 'app' %}
  test:unit:
    enabled: {{ inputs.unit_test | default(value=false) }}

  test:integration:
    enabled: {{ inputs.integration_test | default(value=false) }}

  test:
    enabled: true

  build:
    enabled: true

  publish:
    enabled: {{ inputs.published }}
{% endif %}
{% if inputs.package_type == 'e2e-tests' %}
  test:e2e:
    enabled: {{ inputs.package_type == 'e2e-tests' }}
{% endif %}
{% if inputs.package_type == 'service-tests' %}
  test:service:
    enabled: {{ inputs.package_type == 'service-tests' }}
{% endif %}
{% if inputs.package_type == 'lib' or inputs.package_type == 'script' or inputs.package_type == 'app' %}
meta:
  publish: {{ inputs.published }}
{% endif %}
