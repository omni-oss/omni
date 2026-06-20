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
                    `${entryName || "gen-sdk-core"}.${format === "cjs" ? "cjs" : "mjs"}`,
                name: "GenSdkCore",
            },
        },
    },
});
