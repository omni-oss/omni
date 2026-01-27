import baseConfig from "@omni-oss/vite-config/library";
import { mergeConfig, type UserConfig } from "vite";
{% if prompts.package_type == "lib" %}
import { dependencies } from "./package.json";
{% endif %}

export default mergeConfig(baseConfig, {
    build: {
        minify: "esbuild",
        lib: {
            entry: "src/index.ts",
            formats: ["es", "cjs"],
            fileName: (format, entryName) =>
                `${entryName || "{{ prompts.package_name | kebab_case }}"}.${format === "cjs" ? "cjs" : "mjs"}`,
            name: "{{ prompts.package_name | upper_camel_case }}",
        },
        {% if prompts.package_type == "lib" %}
        rollupOptions: {
            external: Object.keys(dependencies),
        },
        {% endif %}
    },
} satisfies UserConfig);
