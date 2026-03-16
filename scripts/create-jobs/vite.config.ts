import { createConfig } from "@omni-oss/vite-config/script";
import packageJson from "./package.json";

export default createConfig({
    package: packageJson,
    overrides: {
        build: {
            lib: {
                entry: {
                    "create-jobs": "src/cli/index.ts",
                    index: "src/index.ts",
                },

                formats: ["es", "cjs"],
                fileName: (format, entryName) =>
                    `${entryName || "create-jobs"}.${format === "cjs" ? "cjs" : "mjs"}`,
                name: "CreateJobs",
            },
        },
    },
});
