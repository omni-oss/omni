{
    "name": "@omni-oss/{{ prompts.package_name }}",
    "description": "{{ prompts.package_description }}",
    "version": "{{ prompts.package_version }}",
    {% if prompts.package_type == 'script' %}
    "bin": "./dist/cli.js",
    {% endif %}
    "exports": {
        ".": {
            "import": {
                "development": "./src/index.ts",
                "default": "./dist/{{ prompts.package_name }}.mjs"
            },
            "require": {
                "development": "./src/index.ts",
                "default": "./dist/{{ prompts.package_name }}.cjs"
            },
            "types": "./dist/index.d.ts"
        }
    },
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
        "@types/bun": "^1.3.6",
        {% endif %}
        "typescript": "^5.9.3"
    }
}
