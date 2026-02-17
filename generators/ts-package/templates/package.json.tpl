{
    "name": "@omni-oss/{{ prompts.package_name }}",
    "description": "{{ prompts.package_description }}",
    "version": "{{ prompts.package_version }}",
    {% if prompts.package_type == 'script' %}
    "bin": "./dist/{{ prompts.package_name }}.js",
    {% endif %}
    {% if prompts.package_type == 'lib' or (prompts.package_type == 'script' and prompts.script_can_be_used_as_lib) %}
    "exports": {
        ".": {
            "types": "./dist/index.d.ts",
            "import": {
                "development": "./src/index.ts",
                "default": "./dist/index.mjs"
            },
            "require": {
                "development": "./src/index.ts",
                "default": "./dist/index.cjs"
            }
        }
    },
    {% endif %}
    {% if prompts.published %}
    "publishConfig": {
        "access": "{{ prompts.publish_access }}"
    }
    {% endif %}
    "dependencies": {
        {% if prompts.package_type == "script" %}
        "zod": "catalog:",
        "commander": "catalog:",
        "@commander-js/extra-typings": "catalog:"
        {% endif %}
    },
    "devDependencies": {
        "@omni-oss/tsconfig": "workspace:^",
        "@omni-oss/vite-config": "workspace:^",
        "@omni-oss/vitest-config": "workspace:^",
        "vite": "catalog:",
        "vitest": "catalog:",
        {% if prompts.package_type == "script" %}
        "@types/node": "catalog:",
            {% if prompts.shebang_executor == 'bun' %}
        "@types/bun": "catalog:",
            {% elif prompts.shebang_executor == 'deno' %}
        "@types/deno": "catalog:",
            {% endif %}
        {% endif %}
        "typescript": "catalog:"
    }
}
