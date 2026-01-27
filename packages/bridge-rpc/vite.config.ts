import baseConfig from "@omni-oss/vite-config/library";
import { mergeConfig, type UserConfig } from "vite";
import { dependencies } from "./package.json";

export default mergeConfig(baseConfig, {
    build: {
        minify: "esbuild",
        lib: {
            entry: {
                index: "src/index.ts",
                frame: "src/bridge/frame.ts",
                client: "src/bridge/client/index.ts",
                server: "src/bridge/server/index.ts",
            },
            formats: ["es", "cjs"],
            fileName: (format, entryName) =>
                `${entryName || "bridge-rpc"}.${format === "cjs" ? "cjs" : "mjs"}`,
            name: "BridgeRpc",
        },
        rollupOptions: {
            external: Object.keys(dependencies),
        },
    },
} satisfies UserConfig);
