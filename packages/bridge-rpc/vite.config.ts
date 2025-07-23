import baseConfig from "@repo/vite-config/base";
import { mergeConfig, type UserConfig } from "vite";

export default mergeConfig(baseConfig, {
    build: {
        lib: {
            entry: "src/index.ts",
            formats: ["es", "cjs"],
            fileName: (format) =>
                `bridge-rpc.${format === "cjs" ? "cjs" : "mjs"}`,
            name: "BridgeRpc",
        },
    },
} satisfies UserConfig);
