import baseConfig from "@omni-oss/vite-config/library";
import { mergeConfig, type UserConfig } from "vite";

export default mergeConfig(baseConfig, {
    build: {
        minify: "esbuild",
        lib: {
            entry: "src/index.ts",
            formats: ["es", "cjs"],
            fileName: (format) =>
                `{{ prompts.package_name | kebab_case }}.${format === "cjs" ? "cjs" : "mjs"}`,
            name: "{{ prompts.package_name | upper_camel_case }}",
        },
    },
} satisfies UserConfig);
