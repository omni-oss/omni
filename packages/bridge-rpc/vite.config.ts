import { createConfig } from "@omni-oss/vite-config/library";
import packageJson from "./package.json";

export default createConfig({
    package: packageJson,
    overrides: {
        build: {
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
        },
    },
});
