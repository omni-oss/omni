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
                `${entryName || "bridge-rpc-router"}.${format === "cjs" ? "cjs" : "mjs"}`,
            name: "BridgeRpcRouter",
        },
        rollupOptions: {
            external: Object.keys(dependencies),
        },
    },
} satisfies UserConfig);
