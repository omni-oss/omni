import baseConfig from "@omni-oss/vite-config/library";
import { mergeConfig, type UserConfig } from "vite";

export default mergeConfig(baseConfig, {
    build: {
        minify: "esbuild",
        lib: {
            entry: "src/index.ts",
            formats: ["es", "cjs"],
            fileName: (format) =>
                `orcs-api-test.${format === "cjs" ? "cjs" : "mjs"}`,
            name: "OrcsApiTest",
        },
    },
} satisfies UserConfig);
