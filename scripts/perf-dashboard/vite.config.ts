import { createConfig } from "@omni-oss/vite-config/script";

import packageJson from "./package.json";

export default createConfig({
    package: packageJson,

    generateTypes: true,
    externalizeDeps: true,

    overrides: {
        build: {
            lib: {
                entry: {
                    "perf-dashboard-cli": "src/cli/index.ts",

                    index: "src/index.ts",
                },
                formats: ["es"],
                fileName: (format, entryName) =>
                    `${entryName || "perf-dashboard"}.${format === "cjs" ? "cjs" : "mjs"}`,
                name: "PerfDashboard",
            },
        },
    },
});
