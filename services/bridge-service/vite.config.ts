import { createConfig } from "@omni-oss/vite-config/app";

import packageJson from "./package.json";

export default createConfig({
    package: packageJson,
    externalizeDeps: {
        nodeBuiltIns: true,
        denoBuiltIns: true,
        bunBuiltIns: true,
    },
    overrides: {
        build: {
            rolldownOptions: {
                platform: "node",
            },
            lib: {
                entry: {
                    "bridge-service-cli": "src/entrypoint/cli.ts",
                    index: "src/index.ts",
                },

                formats: ["es"],
                fileName: (format, entryName) =>
                    `${entryName || "bridge-service"}.${format === "cjs" ? "cjs" : "mjs"}`,
                name: "BridgeService",
            },
        },
    },
});
