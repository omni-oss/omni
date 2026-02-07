import createBaseConfig from "@omni-oss/vite-config/script";
import { mergeConfig, type UserConfig } from "vite";
import { dependencies } from "./package.json";

const baseConfig = createBaseConfig({
    generateTypes: true,
});

const externalNodeDeps = ["node:path"];

export default mergeConfig(baseConfig, {
    build: {
        minify: "esbuild",
        lib: {
            entry: {
                "set-verson": "src/cli/index.ts",
                index: "src/index.ts",
            },

            formats: ["es", "cjs"],
            fileName: (format, entryName) =>
                `${entryName || "set-version"}.${format === "cjs" ? "cjs" : "mjs"}`,
            name: "SetVersion",
        },

        rollupOptions: {
            external: [...Object.keys(dependencies), ...externalNodeDeps],
        },
    },
} satisfies UserConfig);
