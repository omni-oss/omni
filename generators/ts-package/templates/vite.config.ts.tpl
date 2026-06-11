{% if prompts.package_type == "lib" %}
import { createConfig } from "@omni-oss/vite-config/library";
{% elif prompts.package_type == "script" %}
import { createConfig } from "@omni-oss/vite-config/script";
{% elif prompts.package_type == "app" %}
import { createConfig } from "@omni-oss/vite-config/app";
{% elif prompts.package_type == "e2e-tests" or prompts.package_type == "service-tests" %}
import { createConfig } from "@omni-oss/vite-config/test";
{% endif %}
{% set exclude_deps_from_bundle = prompts.package_type == "lib" or (prompts.package_type == "script" and prompts.script_can_be_used_as_lib) %}
import packageJson from "./package.json";

export default createConfig({
    package: packageJson,
    {% if prompts.package_type == "script" %}
    generateTypes: {{ prompts.script_can_be_used_as_lib }},
    externalizeDeps: {{ exclude_deps_from_bundle }},
    {% endif %}
    overrides: {
        build: {
            lib: {
                entry: {
                    {% if prompts.package_type == "script" %}
                    "{{ prompts.package_name }}-cli": "src/cli/index.ts",
                    {% endif %}
                    "index": "src/index.ts",
                },
                formats: ["es"],
                fileName: (format, entryName) =>
                    `${entryName || "{{ prompts.package_name }}"}.${format === "cjs" ? "cjs" : "mjs"}`,
                name: "{{ prompts.package_name | upper_camel_case }}",
            },
        },
    },
});
