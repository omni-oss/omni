# yaml-language-server: $schema=https://raw.githubusercontent.com/omni-oss/json-schemas/refs/heads/main/project.json
name: "@omni-oss/{{ prompts.package_name }}"

extends:
{% if prompts.package_type == 'lib' %}
  - "@workspace/omni/presets/ts-vite-lib.omni.yaml"
{% elif prompts.package_type == 'app' %}
  - "@workspace/omni/presets/ts-vite-app.omni.yaml"
{% elif prompts.package_type == 'script' %}
  - "@workspace/omni/presets/ts-vite-script.omni.yaml"
{% elif prompts.package_type == 'e2e-tests' or prompts.package_type == 'service-tests' %}
  - "@workspace/omni/presets/ts-vite-test.omni.yaml"
{% endif %}

tasks:
{% if prompts.package_type == 'lib' or prompts.package_type == 'script' or prompts.package_type == 'app' %}
  test:unit:
    enabled: {{ prompts.unit_test | default(value=false) }}

  test:integration:
    enabled: {{ prompts.integration_test | default(value=false) }}

  test:
    enabled: true

  build:
    enabled: true

  publish:
    enabled: {{ prompts.published }}
{% endif %}
{% if prompts.package_type == 'e2e-tests' %}
  test:e2e:
    enabled: {{ prompts.package_type == 'e2e-tests' }}
{% endif %}
{% if prompts.package_type == 'service-tests' %}
  test:service:
    enabled: {{ prompts.package_type == 'service-tests' }}
{% endif %}
{% if prompts.package_type == 'lib' or prompts.package_type == 'script' or prompts.package_type == 'app' %}
meta:
  publish: {{ prompts.published }}
{% endif %}
