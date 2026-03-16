import { createConfig } from "@omni-oss/vite-config/library";
import packageJson from "./package.json";

export default createConfig({
    package: packageJson,
    overrides: {
        build: {
            lib: {
                entry: "src/index.ts",
                formats: ["es", "cjs"],
                fileName: (format, entryName) =>
                    `${entryName || "bridge-rpc-router"}.${format === "cjs" ? "cjs" : "mjs"}`,
                name: "BridgeRpcRouter",
            },
        },
    },
});
