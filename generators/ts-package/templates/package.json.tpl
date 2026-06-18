{
    "name": "{{ vars.full_package_name }}",
    "description": "{{ inputs.package_description }}",
    "version": "{{ inputs.package_version }}",
    {% if inputs.package_type == 'script' %}
    "bin": "./dist/{{ inputs.package_name }}-cli.js",
    {% endif %}
    {% if inputs.package_type == 'lib' or (inputs.package_type == 'script' and inputs.script_can_be_used_as_lib) %}
    "exports": {
        ".": {
            "types": "./dist/index.d.ts",
            "import": {
                "development": "./src/index.ts",
                "default": "./dist/index.mjs"
            },
            "require": {
                "development": "./src/index.ts",
                "default": "./dist/index.mjs"
            }
        }
    },
    {% endif %}
    {% if inputs.published %}
    "publishConfig": {
        "access": "{{ inputs.publish_access }}"
    },
    {% endif %}
    "dependencies": {
        {% if inputs.package_type == "script" %}
        "commander": "catalog:",
        "@commander-js/extra-typings": "catalog:",
        {% endif %}
        "@omni-oss/log": "workspace:^",
        "zod": "catalog:"
    },
    "devDependencies": {
        "@omni-oss/tsconfig": "workspace:^",
        "@omni-oss/vite-config": "workspace:^",
        "@omni-oss/vitest-config": "workspace:^",
        "vite": "catalog:",
        "vitest": "catalog:",
        {% if inputs.package_type == 'script' %}
        "@types/node": "catalog:",
            {% if inputs.shebang_executor == 'bun' %}
        "@types/bun": "catalog:",
            {% elif inputs.shebang_executor == 'deno' %}
        "@types/deno": "catalog:",
            {% endif %}
        {% endif %}
        "typescript": "catalog:"
    }
}
