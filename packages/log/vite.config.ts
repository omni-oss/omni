import { createConfig } from "@omni-oss/vite-config/library";

import packageJson from "./package.json";

export default createConfig({
    package: packageJson,

    overrides: {
        build: {
            lib: {
                entry: {
                    index: "./src/index.ts",
                    core: "./src/core/index.ts",
                    logtape: "./src/logtape/index.ts",
                },

                formats: ["es"],
                fileName: (format, entryName) =>
                    `${entryName || "log"}.${format === "cjs" ? "cjs" : "mjs"}`,
                name: "Log",
            },
        },
    },
});
