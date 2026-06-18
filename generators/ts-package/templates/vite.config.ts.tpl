{% if inputs.package_type == "lib" %}
import { createConfig } from "@omni-oss/vite-config/library";
{% elif inputs.package_type == "script" %}
import { createConfig } from "@omni-oss/vite-config/script";
{% elif inputs.package_type == "app" %}
import { createConfig } from "@omni-oss/vite-config/app";
{% elif inputs.package_type == "e2e-tests" or inputs.package_type == "service-tests" %}
import { createConfig } from "@omni-oss/vite-config/test";
{% endif %}
{% set exclude_deps_from_bundle = inputs.package_type == "lib" or (inputs.package_type == "script" and inputs.script_can_be_used_as_lib) %}
import packageJson from "./package.json";

export default createConfig({
    package: packageJson,
    {% if inputs.package_type == "script" %}
    generateTypes: {{ inputs.script_can_be_used_as_lib }},
    externalizeDeps: {{ exclude_deps_from_bundle }},
    {% endif %}
    overrides: {
        build: {
            lib: {
                entry: {
                    {% if inputs.package_type == "script" %}
                    "{{ inputs.package_name }}-cli": "src/cli/index.ts",
                    {% endif %}
                    "index": "src/index.ts",
                },
                formats: ["es"],
                fileName: (format, entryName) =>
                    `${entryName || "{{ inputs.package_name }}"}.${format === "cjs" ? "cjs" : "mjs"}`,
                name: "{{ inputs.package_name | upper_camel_case }}",
            },
        },
    },
});
