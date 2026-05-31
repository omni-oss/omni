import { createConfig } from "@omni-oss/vite-config/library";

import packageJson from "./package.json";

export default createConfig({
    package: packageJson,

    overrides: {
        build: {
            lib: {
                entry: {
                    index: "./src/index.ts",
                    "exec-script": "./src/exec-script/index.ts",
                },

                formats: ["es", "cjs"],
                fileName: (format, entryName) =>
                    `${entryName || "bridge-rpc-services"}.${format === "cjs" ? "cjs" : "mjs"}`,
                name: "BridgeRpcServices",
            },
        },
    },
});
