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
                    "exec-generator-script":
                        "./src/exec-generator-script/index.ts",
                    "rpc-system": "./src/rpc-system/index.ts",
                },

                formats: ["es"],
                fileName: (format, entryName) =>
                    `${entryName || "bridge-rpc-services"}.${format === "cjs" ? "cjs" : "mjs"}`,
                name: "BridgeRpcServices",
            },
        },
    },
});
