import { createConfig } from "@omni-oss/vite-config/script";
import packageJson from "./package.json";

export default createConfig({
    package: packageJson,
    generateTypes: true,
    overrides: {
        build: {
            lib: {
                entry: {
                    "set-version": "src/cli/index.ts",
                    index: "src/index.ts",
                },

                formats: ["es", "cjs"],
                fileName: (format, entryName) =>
                    `${entryName || "set-version"}.${format === "cjs" ? "cjs" : "mjs"}`,
                name: "SetVersion",
            },
        },
    },
});
