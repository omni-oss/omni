import createBaseConfig from "@omni-oss/vite-config/script";
import { mergeConfig, type UserConfig } from "vite";
import { dependencies } from "./package.json";

const baseConfig = createBaseConfig({
    generateTypes: true,
});

export default mergeConfig(baseConfig, {
    build: {
        minify: "esbuild",
        lib: {
            entry: {
                cli: "src/cli/index.ts",
                "set-version": "src/index.ts",
            },

            formats: ["es", "cjs"],
            fileName: (format, entryName) =>
                `${entryName || "set-version"}.${format === "cjs" ? "cjs" : "mjs"}`,
            name: "SetVersion",
        },

        rollupOptions: {
            external: [...Object.keys(dependencies), "node:path"],
        },
    },
} satisfies UserConfig);
