import baseConfig from "@omni-oss/vite-config/library";
import { mergeConfig, type UserConfig } from "vite";
import { dependencies } from "./package.json";

export default mergeConfig(baseConfig, {
    build: {
        minify: "esbuild",
        lib: {
            entry: "src/index.ts",
            formats: ["es", "cjs"],
            fileName: (format, entryName) =>
                `${entryName || "system-interface"}.${format === "cjs" ? "cjs" : "mjs"}`,
            name: "SystemInterface",
        },
        target: ["node16"],
        rollupOptions: {
            external: [
                ...Object.keys(dependencies),
                "node:fs",
                "node:fs/promises",
                "node:path",
                "node:process",
            ],
        },
    },
} satisfies UserConfig);
