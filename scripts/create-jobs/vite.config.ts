import createBaseConfig from "@omni-oss/vite-config/script";
import { mergeConfig, type UserConfig } from "vite";
import { dependencies } from "./package.json";

const baseConfig = createBaseConfig({
    generateTypes: true,
});

const externalNodeDeps = ["node:path", "node:fs", "node:fs/promises"];

export default mergeConfig(baseConfig, {
    build: {
        minify: "esbuild",
        lib: {
            entry: {
                "create-jobs": "src/cli/index.ts",
                index: "src/index.ts",
            },

            formats: ["es", "cjs"],
            fileName: (format, entryName) =>
                `${entryName || "create-jobs"}.${format === "cjs" ? "cjs" : "mjs"}`,
            name: "CreateJobs",
        },

        rollupOptions: {
            external: [...Object.keys(dependencies), ...externalNodeDeps],
        },
    },
} satisfies UserConfig);
