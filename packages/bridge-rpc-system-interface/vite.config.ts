import { createConfig } from "@omni-oss/vite-config/library";

import packageJson from "./package.json";

export default createConfig({
    package: packageJson,
    overrides: {
        build: {
            lib: {
                entry: {
                    index: "src/index.ts",
                },

                formats: ["es"],
                fileName: (format, entryName) =>
                    `${entryName || "bridge-rpc-system-interface"}.${format === "cjs" ? "cjs" : "mjs"}`,
                name: "BridgeRpcSystemInterface",
            },
        },
    },
});
