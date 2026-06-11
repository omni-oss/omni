import { createConfig } from "@omni-oss/vite-config/library";

import packageJson from "./package.json";

export default createConfig({
    package: packageJson,

    overrides: {
        build: {
            lib: {
                entry: {
                    index: "./src/index.ts",
                    body: "./src/body/index.ts",
                    server: "./src/server/index.ts",
                },
                formats: ["es"],
                fileName: (format, entryName) =>
                    `${entryName || "bridge-rpc-utils"}.${format === "cjs" ? "cjs" : "mjs"}`,
                name: "BridgeRpcUtils",
            },
        },
    },
});
