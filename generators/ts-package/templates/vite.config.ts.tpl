{% if prompts.package_type == "lib" %}
import baseConfig from "@omni-oss/vite-config/library";
{% elif prompts.package_type == "script" %}
import createBaseConfig from "@omni-oss/vite-config/script";
{% elif prompts.package_type == "app" %}
import baseConfig from "@omni-oss/vite-config/app";
{% endif %}
import { mergeConfig, type UserConfig } from "vite";
{% if prompts.package_type == "lib" %}
import { dependencies } from "./package.json";
{% endif %}

{% if prompts.package_type == "script" %}
const baseConfig = createBaseConfig({
    generateTypes: {{ prompts.script_can_be_used_as_lib }},
});
{% endif %}

export default mergeConfig(baseConfig, {
    build: {
        minify: "esbuild",
        lib: {
            {% if prompts.package_type == "script" %}
            entry: {
                cli: "src/cli/index.ts",
                "{{ prompts.package_name }}": "src/index.ts",
            },
            {% else %}
            entry: "src/index.ts",
            {% endif %}
            formats: ["es", "cjs"],
            fileName: (format, entryName) =>
                `${entryName || "{{ prompts.package_name }}"}.${format === "cjs" ? "cjs" : "mjs"}`,
            name: "{{ prompts.package_name | upper_camel_case }}",
        },
        {% if prompts.package_type == "lib" or prompts.package_type == "script" %}
        rollupOptions: {
            external: Object.keys(dependencies),
        },
        {% endif %}
    },
} satisfies UserConfig);
