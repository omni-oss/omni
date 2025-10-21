import baseConfig from "@omni-oss/vite-config/library";
import { mergeConfig, type UserConfig } from "vite";

export default mergeConfig(baseConfig, {
    build: {
        minify: "esbuild",
        lib: {
            entry: "src/index.ts",
            formats: ["es", "cjs"],
            fileName: (format) =>
                `bridge-rpc.${format === "cjs" ? "cjs" : "mjs"}`,
            name: "BridgeRpc",
        },
    },
} satisfies UserConfig);
