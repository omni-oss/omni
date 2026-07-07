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
                    "task-bench-cli": "src/cli/index.ts",
                    index: "src/index.ts",
                },
                formats: ["es"],
                fileName: (format, entryName) =>
                    `${entryName || "task-bench"}.${format === "cjs" ? "cjs" : "mjs"}`,
                name: "TaskBench",
            },
        },
    },
});
