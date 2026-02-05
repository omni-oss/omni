{
    "name": "@omni-oss/{{ prompts.package_name }}",
    "description": "{{ prompts.package_description }}",
    "version": "{{ prompts.package_version }}",
    {% if prompts.package_type == 'script' %}
    "bin": "./dist/cli.js",
    {% endif %}
    {% if prompts.package_type == 'lib' or (prompts.package_type == 'script' and prompts.script_can_be_used_as_lib) %}
    "exports": {
        ".": {
            "types": "./dist/index.d.ts",
            "import": {
                "development": "./src/index.ts",
                "default": "./dist/{{ prompts.package_name }}.mjs"
            },
            "require": {
                "development": "./src/index.ts",
                "default": "./dist/{{ prompts.package_name }}.cjs"
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
        "zod": "^4.3.6",
        "commander": "^14.0.2",
        "@commander-js/extra-typings": "14.0.0"
        {% endif %}
    },
    "devDependencies": {
        "@omni-oss/tsconfig": "workspace:*",
        "@omni-oss/vite-config": "workspace:*",
        "@omni-oss/vitest-config": "workspace:*",
        "vite": "^7.2.2",
        "vitest": "^4.0.8",
        {% if prompts.package_type == "script" %}
        "@types/node": "24.10.1",
            {% if prompts.shebang_executor == 'bun' %}
        "@types/bun": "^1.3.8",
            {% elif f prompts.shebang_executor == 'deno' %}
        "@types/deno": "^2.5.0",
            {% endif %}
        {% endif %}
        "typescript": "^5.9.3"
    }
}
